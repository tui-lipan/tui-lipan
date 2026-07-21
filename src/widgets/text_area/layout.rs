use std::hash::{Hash, Hasher};
use std::sync::Arc;

use rustc_hash::FxHasher;

use crate::core::node::{NodeKind, ScrollbarZone};
use crate::style::ScrollbarVariant;
use crate::utils::prepared_text::{
    PreparedText, SegmentKind, layout_lines, layout_lines_with_caret, prepare_text,
};
use crate::utils::text::{
    VirtualTextInsertion, char_visual_width, str_visual_width_with_tabs, visual_col_with_virtual,
};
#[cfg(feature = "diff-view")]
use crate::widgets::diff_view::{
    compute_split_wrap_padding, compute_split_wrap_padding_from_heights, peer_pass1_source_heights,
    peer_simulated_content_width, record_pass1_source_heights, split_wrap_layout_pass,
    split_wrap_pane_widths, split_wrap_scrollbar_cols_pair,
};
use crate::widgets::text_area::TextArea;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextAreaVisualKey {
    pub value_hash: u64,
    pub peer_hash: u64,
    pub inner_w: u16,
    pub wrap: bool,
    pub line_numbers: bool,
    pub min_line_number_width: u8,
    /// Whether a standalone vertical scrollbar column is reserved.
    pub reserve_scrollbar_col: bool,
    /// Extra empty cells reserved before a standalone vertical scrollbar.
    pub reserve_scrollbar_gap: u16,
    pub read_only: bool,
    pub caret: Option<usize>,
    /// Tab stop used by tab-aware visual width and wrapping.
    pub tab_stop: u8,
    /// Placeholder width for inline image sentinels (0 when not in Inline mode or no images).
    pub sentinel_ph_width: usize,
    /// Number of inline images (0 when not applicable).
    pub sentinel_count: usize,
    /// Hash of custom sentinel widths (0 when no custom sentinels).
    pub custom_sentinel_hash: u64,
    /// Hash of virtual text content/anchors/styles.
    pub virtual_text_hash: u64,
    /// Custom gutter column width (0 = not set, use line_numbers-derived width).
    pub gutter_col_width: u16,
    /// Fixed empty cells between gutter and text content.
    pub gutter_gap: u16,
    #[cfg(feature = "diff-view")]
    pub split_wrap_pane_widths: Option<(u16, u16)>,
    #[cfg(feature = "diff-view")]
    pub split_wrap_scrollbar_cols: Option<(u16, u16)>,
    #[cfg(feature = "diff-view")]
    pub split_wrap_layout_pass: u8,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TextAreaGeometry {
    pub inner_w: u16,
    pub inner_h: u16,
    pub gutter_width: usize,
    pub content_width: usize,
    pub total_visual_lines: usize,
    pub cursor_visual_line: usize,
    pub max_line_width: usize,
    pub viewport_height: u16,
    pub h_scrollbar_visible: bool,
    pub v_scrollbar_visible: bool,
    pub scrollbar_zones: Vec<ScrollbarZone>,
}

impl TextAreaGeometry {
    /// Content viewport height after accounting for a standalone horizontal scrollbar row.
    pub fn content_viewport_h(&self, h_scrollbar_over_border: bool) -> u16 {
        if self.viewport_height > 0 {
            self.viewport_height
        } else if self.h_scrollbar_visible && !h_scrollbar_over_border {
            self.inner_h.saturating_sub(1)
        } else {
            self.inner_h
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextAreaVisualCacheEntry {
    pub key: TextAreaVisualKey,
    pub geometry: TextAreaGeometry,
    pub lines: Vec<TextAreaVisualLine>,
    #[cfg(feature = "diff-view")]
    pub own_source_heights: Option<Arc<Vec<u16>>>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TextAreaVisualCache {
    entries: Vec<TextAreaVisualCacheEntry>,
    prepared_entries: Vec<TextAreaPreparedCacheEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextAreaPreparedKey {
    value_hash: u64,
    sentinel_ph_width: usize,
    sentinel_count: usize,
    custom_sentinel_hash: u64,
    tab_stop: u8,
}

#[derive(Clone, Debug)]
struct TextAreaPreparedCacheEntry {
    key: TextAreaPreparedKey,
    prepared: Arc<PreparedText>,
}

impl TextAreaVisualCache {
    pub(crate) fn get_lines(&self, key: &TextAreaVisualKey) -> Option<&[TextAreaVisualLine]> {
        self.entries
            .iter()
            .find(|entry| entry.key == *key)
            .map(|entry| entry.lines.as_slice())
    }

    pub(crate) fn get_lines_cloned(
        &self,
        key: &TextAreaVisualKey,
    ) -> Option<Arc<[TextAreaVisualLine]>> {
        self.get_lines(key).map(Arc::from)
    }

    pub(crate) fn insert_with_lines(
        &mut self,
        key: TextAreaVisualKey,
        geometry: TextAreaGeometry,
        lines: Vec<TextAreaVisualLine>,
        #[cfg(feature = "diff-view")] own_source_heights: Option<Arc<Vec<u16>>>,
    ) {
        if let Some(idx) = self.entries.iter().position(|entry| entry.key == key) {
            self.entries.remove(idx);
        }

        self.entries.push(TextAreaVisualCacheEntry {
            key,
            geometry,
            lines,
            #[cfg(feature = "diff-view")]
            own_source_heights,
        });
        if self.entries.len() > 2 {
            self.entries.remove(0);
        }
    }

    /// Lines from the most recently inserted layout pass.
    ///
    /// Unlike [`Self::get_lines`], this ignores the cache key, so the returned
    /// byte offsets may belong to an older value of the text (e.g. right after
    /// undo/delete, before the next layout pass). Callers slicing the current
    /// text with these offsets must clamp them to char boundaries first (see
    /// `crate::utils::text::clamp_cursor` / `next_char_boundary`), or they can
    /// panic inside multi-byte characters.
    pub(crate) fn latest_lines(&self) -> Option<&[TextAreaVisualLine]> {
        self.entries.last().map(|e| e.lines.as_slice())
    }

    fn prepared_text(
        &mut self,
        key: TextAreaPreparedKey,
        value: &str,
        sentinel: Option<&crate::utils::text::SentinelInfo>,
        tab_stop: u8,
    ) -> Arc<PreparedText> {
        if let Some((idx, _)) = self
            .prepared_entries
            .iter()
            .enumerate()
            .find(|(_, entry)| entry.key == key)
        {
            let entry = self.prepared_entries.remove(idx);
            let prepared = entry.prepared.clone();
            self.prepared_entries.push(entry);
            return prepared;
        }

        let prepared = Arc::new(prepare_text(value, sentinel, tab_stop as usize));
        self.prepared_entries.push(TextAreaPreparedCacheEntry {
            key,
            prepared: prepared.clone(),
        });
        if self.prepared_entries.len() > 2 {
            self.prepared_entries.remove(0);
        }
        prepared
    }
}

pub(crate) fn hash_text(value: &str) -> u64 {
    let mut hasher = FxHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn hash_peer_source_lines(peer_source_lines: Option<&Arc<Vec<Arc<str>>>>) -> u64 {
    let mut hasher = FxHasher::default();
    if let Some(lines) = peer_source_lines {
        lines.len().hash(&mut hasher);
        for line in lines.iter() {
            line.as_ref().hash(&mut hasher);
        }
    } else {
        0usize.hash(&mut hasher);
    }
    hasher.finish()
}

pub(crate) fn logical_line_count(value: &str) -> usize {
    if value.is_empty() {
        1
    } else {
        value.as_bytes().iter().filter(|&&b| b == b'\n').count() + 1
    }
}

pub(crate) fn text_area_cursor_reserve(wrap: bool, read_only: bool) -> u16 {
    u16::from(!wrap && !read_only)
}

pub(crate) struct TextAreaVisualKeyArgs {
    pub inner_w: u16,
    pub wrap: bool,
    pub line_numbers: bool,
    pub min_line_number_width: u8,
    pub scrollbar: bool,
    pub scrollbar_over_border: bool,
    pub scrollbar_gap: u16,
    pub read_only: bool,
    pub cursor: usize,
    pub tab_stop: u8,
    pub sentinel_ph_width: usize,
    pub sentinel_count: usize,
    pub custom_sentinel_hash: u64,
    pub virtual_text_hash: u64,
    pub gutter_col_width: u16,
    pub gutter_gap: u16,
    #[cfg(feature = "diff-view")]
    pub split_wrap_pane_widths: Option<(u16, u16)>,
    #[cfg(feature = "diff-view")]
    pub split_wrap_scrollbar_cols: Option<(u16, u16)>,
    #[cfg(feature = "diff-view")]
    pub split_wrap_layout_pass: u8,
}

pub(crate) fn make_text_area_visual_key(
    value_hash: u64,
    peer_hash: u64,
    args: TextAreaVisualKeyArgs,
) -> TextAreaVisualKey {
    let TextAreaVisualKeyArgs {
        inner_w,
        wrap,
        line_numbers,
        min_line_number_width,
        scrollbar,
        scrollbar_over_border,
        scrollbar_gap,
        read_only,
        cursor,
        tab_stop,
        sentinel_ph_width,
        sentinel_count,
        custom_sentinel_hash,
        virtual_text_hash,
        gutter_col_width,
        gutter_gap,
        #[cfg(feature = "diff-view")]
        split_wrap_pane_widths,
        #[cfg(feature = "diff-view")]
        split_wrap_scrollbar_cols,
        #[cfg(feature = "diff-view")]
        split_wrap_layout_pass,
    } = args;
    TextAreaVisualKey {
        value_hash,
        peer_hash,
        inner_w,
        wrap,
        line_numbers,
        min_line_number_width,
        reserve_scrollbar_col: scrollbar && !scrollbar_over_border,
        reserve_scrollbar_gap: if scrollbar && !scrollbar_over_border {
            scrollbar_gap
        } else {
            0
        },
        read_only,
        caret: (wrap && !read_only).then_some(cursor),
        tab_stop,
        sentinel_ph_width,
        sentinel_count,
        custom_sentinel_hash,
        virtual_text_hash,
        gutter_col_width,
        gutter_gap,
        #[cfg(feature = "diff-view")]
        split_wrap_pane_widths,
        #[cfg(feature = "diff-view")]
        split_wrap_scrollbar_cols,
        #[cfg(feature = "diff-view")]
        split_wrap_layout_pass,
    }
}

pub(crate) fn text_area_gutter_width(
    logical_lines_count: usize,
    line_numbers: bool,
    min_line_number_width: u8,
    gutter_col_width: u16,
) -> u16 {
    if gutter_col_width > 0 {
        gutter_col_width
    } else if line_numbers {
        logical_lines_count
            .to_string()
            .len()
            .max(min_line_number_width as usize) as u16
            + 2
    } else {
        0
    }
}

pub(crate) fn text_area_total_gutter_width(
    logical_lines_count: usize,
    line_numbers: bool,
    min_line_number_width: u8,
    gutter_col_width: u16,
    gutter_gap: u16,
) -> u16 {
    let gutter_width = text_area_gutter_width(
        logical_lines_count,
        line_numbers,
        min_line_number_width,
        gutter_col_width,
    );
    gutter_width.saturating_add(if gutter_width > 0 { gutter_gap } else { 0 })
}

pub(crate) fn text_area_visual_line_for_cursor(
    lines: &[TextAreaVisualLine],
    cursor: usize,
) -> usize {
    for (idx, line) in lines.iter().enumerate() {
        if cursor == line.start && line.continuation {
            return idx;
        }
        if cursor >= line.start && cursor < line.end {
            return idx;
        }
        let next_starts_at_boundary = lines.get(idx + 1).is_some_and(|next| {
            next.line_num == line.line_num && next.continuation && next.start == cursor
        });
        if cursor == line.end && !next_starts_at_boundary {
            return idx;
        }
    }
    lines.len().saturating_sub(1)
}

fn cursor_visual_line_for_cursor(lines: &[TextAreaVisualLine], cursor: usize) -> usize {
    text_area_visual_line_for_cursor(lines, cursor)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextAreaVisualLine {
    pub line_num: usize,
    pub continuation: bool,
    pub start: usize,
    pub end: usize,
    /// Visual column at the left edge within the logical line.
    pub visual_start_col: usize,
    /// Visual column just after this row's rendered content within the logical line.
    pub visual_end_col: usize,
    /// Whether inline virtual text anchored at `start` belongs to this row.
    pub starts_with_virtual_text: bool,
    /// Whether inline virtual text anchored at `end` belongs to this row.
    pub ends_with_virtual_text: bool,
}

fn max_logical_line_width(prepared: &PreparedText) -> usize {
    let mut max_line_width = 0usize;
    let mut current_line_width = 0usize;
    for (seg, width) in prepared.segments.iter().zip(prepared.widths.iter()) {
        if seg.kind == SegmentKind::HardBreak {
            max_line_width = max_line_width.max(current_line_width);
            current_line_width = 0;
        } else {
            current_line_width = current_line_width.saturating_add(*width);
        }
    }
    max_line_width.max(current_line_width)
}

struct VirtualLayoutLineArgs {
    line_num: usize,
    continuation: bool,
    line_start_abs: usize,
    start: usize,
    end: usize,
    visual_start_col: usize,
    visual_end_col: usize,
    starts_with_virtual_text: bool,
    ends_with_virtual_text: bool,
}

fn push_virtual_layout_line(out: &mut Vec<TextAreaVisualLine>, args: VirtualLayoutLineArgs) {
    let VirtualLayoutLineArgs {
        line_num,
        continuation,
        line_start_abs,
        start,
        end,
        visual_start_col,
        visual_end_col,
        starts_with_virtual_text,
        ends_with_virtual_text,
    } = args;
    out.push(TextAreaVisualLine {
        line_num,
        continuation,
        start: line_start_abs.saturating_add(start),
        end: line_start_abs.saturating_add(end),
        visual_start_col,
        visual_end_col,
        starts_with_virtual_text,
        ends_with_virtual_text,
    });
}

pub(crate) struct VirtualTextLayoutCtx<'a> {
    pub line_start_abs: usize,
    pub line_num: usize,
    pub wrap: bool,
    pub content_width: usize,
    pub caret: Option<usize>,
    pub sentinel: Option<&'a crate::utils::text::SentinelInfo>,
    pub tab_stop: usize,
    pub insertions: &'a [VirtualTextInsertion],
}

pub(crate) fn layout_line_with_inline_virtual_text(
    line: &str,
    ctx: VirtualTextLayoutCtx<'_>,
    out: &mut Vec<TextAreaVisualLine>,
) -> usize {
    let VirtualTextLayoutCtx {
        line_start_abs,
        line_num,
        wrap,
        content_width,
        caret,
        sentinel,
        tab_stop,
        insertions,
    } = ctx;
    let max_line_width = visual_col_with_virtual(line, 0, tab_stop, sentinel, insertions);
    let wrap_width = if wrap
        && content_width > 1
        && caret == Some(line_start_abs.saturating_add(line.len()))
        && max_line_width == content_width
    {
        content_width - 1
    } else {
        content_width.max(1)
    };
    if !wrap || max_line_width <= wrap_width {
        push_virtual_layout_line(
            out,
            VirtualLayoutLineArgs {
                line_num,
                continuation: false,
                line_start_abs,
                start: 0,
                end: line.len(),
                visual_start_col: 0,
                visual_end_col: max_line_width,
                starts_with_virtual_text: insertions.iter().any(|insertion| insertion.anchor == 0),
                ends_with_virtual_text: insertions
                    .iter()
                    .any(|insertion| insertion.anchor == line.len()),
            },
        );
        return max_line_width;
    }

    let mut insertion_idx = 0usize;
    let mut row_start = 0usize;
    let mut row_end = 0usize;
    let mut row_start_col = 0usize;
    let mut row_width = 0usize;
    let mut row_has_content = false;
    let mut row_starts_with_virtual = false;
    let mut row_ends_with_virtual = false;
    let mut is_first_row = true;

    let add_unit = |unit_start: usize,
                    unit_end: usize,
                    unit_width: usize,
                    is_virtual: bool,
                    out: &mut Vec<TextAreaVisualLine>,
                    row_start: &mut usize,
                    row_end: &mut usize,
                    row_start_col: &mut usize,
                    row_width: &mut usize,
                    row_has_content: &mut bool,
                    row_starts_with_virtual: &mut bool,
                    row_ends_with_virtual: &mut bool,
                    is_first_row: &mut bool| {
        if *row_has_content && (*row_width).saturating_add(unit_width) > wrap_width {
            push_virtual_layout_line(
                out,
                VirtualLayoutLineArgs {
                    line_num,
                    continuation: !*is_first_row,
                    line_start_abs,
                    start: *row_start,
                    end: *row_end,
                    visual_start_col: *row_start_col,
                    visual_end_col: (*row_start_col).saturating_add(*row_width),
                    starts_with_virtual_text: *row_starts_with_virtual,
                    ends_with_virtual_text: *row_ends_with_virtual,
                },
            );
            *row_start_col = (*row_start_col).saturating_add(*row_width);
            *row_start = unit_start;
            *row_end = unit_start;
            *row_width = 0;
            *row_has_content = false;
            *row_starts_with_virtual = false;
            *row_ends_with_virtual = false;
            *is_first_row = false;
        }

        if !*row_has_content {
            *row_start = unit_start;
            *row_end = unit_start;
            *row_starts_with_virtual = is_virtual;
        }
        *row_width = (*row_width).saturating_add(unit_width);
        *row_end = unit_end;
        *row_has_content = true;
        *row_ends_with_virtual = is_virtual;
    };

    for (idx, ch) in line.char_indices() {
        while let Some(insertion) = insertions.get(insertion_idx) {
            if insertion.anchor != idx {
                break;
            }
            add_unit(
                idx,
                idx,
                insertion.width,
                true,
                out,
                &mut row_start,
                &mut row_end,
                &mut row_start_col,
                &mut row_width,
                &mut row_has_content,
                &mut row_starts_with_virtual,
                &mut row_ends_with_virtual,
                &mut is_first_row,
            );
            insertion_idx = insertion_idx.saturating_add(1);
        }
        let char_width = if ch == '\t' && tab_stop > 0 {
            let logical_col = row_start_col.saturating_add(row_width);
            tab_stop - (logical_col % tab_stop)
        } else {
            char_visual_width(ch, sentinel)
        };
        add_unit(
            idx,
            idx.saturating_add(ch.len_utf8()),
            char_width,
            false,
            out,
            &mut row_start,
            &mut row_end,
            &mut row_start_col,
            &mut row_width,
            &mut row_has_content,
            &mut row_starts_with_virtual,
            &mut row_ends_with_virtual,
            &mut is_first_row,
        );
    }

    while let Some(insertion) = insertions.get(insertion_idx) {
        if insertion.anchor > line.len() {
            break;
        }
        add_unit(
            line.len(),
            line.len(),
            insertion.width,
            true,
            out,
            &mut row_start,
            &mut row_end,
            &mut row_start_col,
            &mut row_width,
            &mut row_has_content,
            &mut row_starts_with_virtual,
            &mut row_ends_with_virtual,
            &mut is_first_row,
        );
        insertion_idx = insertion_idx.saturating_add(1);
    }

    if row_has_content {
        push_virtual_layout_line(
            out,
            VirtualLayoutLineArgs {
                line_num,
                continuation: !is_first_row,
                line_start_abs,
                start: row_start,
                end: row_end,
                visual_start_col: row_start_col,
                visual_end_col: row_start_col.saturating_add(row_width),
                starts_with_virtual_text: row_starts_with_virtual,
                ends_with_virtual_text: row_ends_with_virtual,
            },
        );
    } else {
        push_virtual_layout_line(
            out,
            VirtualLayoutLineArgs {
                line_num,
                continuation: false,
                line_start_abs,
                start: 0,
                end: 0,
                visual_start_col: 0,
                visual_end_col: 0,
                starts_with_virtual_text: false,
                ends_with_virtual_text: false,
            },
        );
    }

    max_line_width
}

fn cursor_visual_line_for_wrapped_lines(lines: &[TextAreaVisualLine], cursor: usize) -> usize {
    text_area_visual_line_for_cursor(lines, cursor)
}

pub fn measure_text_area(text_area: &TextArea) -> (u16, u16) {
    let value = text_area.value.as_ref();
    let sentinel = text_area.sentinel_info();
    let mut w = 0usize;
    let mut h = 0usize;

    let tab_stop = text_area.tab_display_width as usize;
    for line in value.lines() {
        w = w.max(str_visual_width_with_tabs(
            line,
            sentinel.as_ref(),
            0,
            tab_stop,
        ));
        h = h.saturating_add(1);
    }
    if h == 0 {
        h = 1;
    }

    if !text_area.read_only {
        w = w.saturating_add(1);
    }

    let lines_count = logical_line_count(value);
    w = w.saturating_add(text_area_total_gutter_width(
        lines_count,
        text_area.line_numbers,
        text_area.min_line_number_width,
        text_area.gutter_col_width,
        text_area.gutter_gap,
    ) as usize);

    w = w.saturating_add(text_area.padding.horizontal() as usize);
    let mut final_h = h.saturating_add(text_area.padding.vertical() as usize);

    if text_area.border {
        w = w.saturating_add(2);
        final_h = final_h.saturating_add(2);
    }

    let w = w.min(u16::MAX as usize) as u16;
    let h = final_h.min(u16::MAX as usize) as u16;
    (w, h)
}

pub fn measure_text_area_constrained(text_area: &TextArea, max_width: Option<u16>) -> (u16, u16) {
    let (natural_w, natural_h) = measure_text_area(text_area);

    let effective_max_w = match (text_area.width, max_width) {
        (crate::style::Length::Px(px), Some(mw)) => Some(mw.min(px)),
        (crate::style::Length::Px(px), None) => Some(px),
        (crate::style::Length::Percent(percent), Some(mw)) => {
            Some(crate::style::Length::Percent(percent).resolve(mw, mw))
        }
        (crate::style::Length::Percent(_), None) => None,
        (crate::style::Length::Flex(_) | crate::style::Length::Auto, _) => max_width,
    };

    let Some(max_w) = effective_max_w else {
        return (natural_w, natural_h);
    };

    let is_fixed_width = matches!(
        text_area.width,
        crate::style::Length::Px(_) | crate::style::Length::Percent(_)
    );

    if !text_area.wrap {
        let w = if is_fixed_width {
            max_w
        } else {
            natural_w.min(max_w)
        };

        let mut h = natural_h;
        if matches!(text_area.height, crate::style::Length::Auto) {
            let border_w: u16 = if text_area.border { 2 } else { 0 };
            let inner_w = w
                .saturating_sub(border_w)
                .saturating_sub(text_area.padding.horizontal());
            let value_hash = hash_text(text_area.value.as_ref());
            let geo =
                calculate_text_area_visual_metrics(text_area, inner_w, true, value_hash, None);
            let h_scrollbar_over_border = text_area.h_scrollbar
                && matches!(text_area.h_scrollbar_variant, ScrollbarVariant::Integrated)
                && text_area.border;
            let h_scrollbar_visible =
                text_area.h_scrollbar && !text_area.wrap && geo.max_line_width > geo.content_width;
            if h_scrollbar_visible && !h_scrollbar_over_border {
                h = h.saturating_add(1);
            }
        }

        return (w, h);
    }

    // For wrapping calculations, use the width that will actually be available.
    // If width is Flex or Auto, the TextArea will expand to fill available space,
    // so we should use max_w for wrapping calculations to ensure consistency
    // with reconcile_text_area which uses the actual allocated rect.
    let layout_w = text_area.width.resolve(max_w, max_w).min(max_w);
    let clamped_w = if is_fixed_width {
        layout_w
    } else {
        natural_w.min(layout_w)
    };

    let border_w: u16 = if text_area.border { 2 } else { 0 };
    let padding_h = text_area.padding.horizontal();
    // Use layout_w (not clamped_w) for inner_w to match actual rendering width
    let inner_w = layout_w.saturating_sub(border_w).saturating_sub(padding_h);

    let value_hash = hash_text(text_area.value.as_ref());
    let reserve_v_scrollbar = !matches!(text_area.height, crate::style::Length::Auto);
    let geo = calculate_text_area_visual_metrics(
        text_area,
        inner_w,
        !reserve_v_scrollbar,
        value_hash,
        None,
    );

    let mut visual_h = geo.total_visual_lines;
    visual_h = visual_h.saturating_add(text_area.padding.vertical() as usize);
    if text_area.border {
        visual_h = visual_h.saturating_add(2);
    }

    (clamped_w, visual_h as u16)
}

pub(crate) fn text_area_pending_vim_search_row(
    text_area: &TextArea,
    node_kind: Option<&NodeKind>,
) -> bool {
    matches!(
        node_kind,
        Some(NodeKind::TextArea(node))
            if text_area.vim_motions
                && !text_area.read_only
                && text_area.on_change.is_some()
                && node.vim_search_feedback.as_ref().is_some_and(|feedback| feedback.pending)
    )
}

pub(crate) fn text_area_auto_height_for_width(
    text_area: &TextArea,
    width: u16,
    pending_vim_search_row: bool,
    parent_h_edge: bool,
) -> u16 {
    let (natural_w, natural_h) = measure_text_area(text_area);
    let resolved_w = if matches!(text_area.width, crate::style::Length::Auto) {
        natural_w.min(width)
    } else {
        width
    };
    let inner_w = resolved_w
        .saturating_sub(if text_area.border { 2 } else { 0 })
        .saturating_sub(text_area.padding.horizontal());
    let value_hash = hash_text(text_area.value.as_ref());
    let geo = calculate_text_area_visual_metrics(text_area, inner_w, true, value_hash, None);

    if text_area.wrap {
        let mut visual_h = geo
            .total_visual_lines
            .saturating_add(usize::from(pending_vim_search_row));
        visual_h = visual_h.saturating_add(text_area.padding.vertical() as usize);
        if text_area.border {
            visual_h = visual_h.saturating_add(2);
        }
        return visual_h.min(u16::MAX as usize) as u16;
    }

    let h_scrollbar_over_border = text_area.h_scrollbar
        && matches!(text_area.h_scrollbar_variant, ScrollbarVariant::Integrated)
        && (text_area.border || parent_h_edge);
    let h_scrollbar_visible =
        text_area.h_scrollbar && geo.max_line_width > geo.content_width && !pending_vim_search_row;
    let mut height = natural_h.saturating_add(u16::from(pending_vim_search_row));
    if h_scrollbar_visible && !h_scrollbar_over_border {
        height = height.saturating_add(1);
    }
    height
}

pub(crate) fn calculate_text_area_visual_metrics(
    text_area: &TextArea,
    inner_w: u16,
    scrollbar_over_border: bool,
    value_hash: u64,
    mut cache: Option<&mut TextAreaVisualCache>,
) -> TextAreaGeometry {
    let value = text_area.value.as_ref();
    let wrap = text_area.wrap;
    let line_numbers = text_area.line_numbers;
    let min_line_number_width = text_area.min_line_number_width;
    let scrollbar = text_area.scrollbar;
    let cursor = text_area.cursor;
    let sentinel = text_area.sentinel_info();
    // Sentinel cache key: image placeholder width + image count + custom sentinel widths hash
    let (sentinel_ph_width, sentinel_count) = sentinel
        .as_ref()
        .and_then(|si| si.image.map(|(_, _, pw)| (pw, text_area.images.len())))
        .unwrap_or((0, 0));
    let custom_sentinel_widths_hash: u64 = {
        use std::hash::{Hash, Hasher};
        let mut h = rustc_hash::FxHasher::default();
        if let Some(si) = sentinel.as_ref()
            && let Some((_, _, ref widths, _)) = si.custom
        {
            widths.hash(&mut h);
        }
        h.finish()
    };
    let virtual_text_hash = super::text_area_virtual_text_hash(&text_area.virtual_texts);
    let cache_key = make_text_area_visual_key(
        value_hash,
        hash_peer_source_lines(text_area.peer_source_lines.as_ref()),
        TextAreaVisualKeyArgs {
            inner_w,
            wrap,
            line_numbers,
            min_line_number_width,
            scrollbar,
            scrollbar_over_border,
            scrollbar_gap: text_area.scrollbar_config.gap,
            read_only: text_area.read_only,
            cursor: text_area.cursor,
            tab_stop: text_area.tab_display_width,
            sentinel_ph_width,
            sentinel_count,
            custom_sentinel_hash: custom_sentinel_widths_hash,
            virtual_text_hash,
            gutter_col_width: text_area.gutter_col_width,
            gutter_gap: text_area.gutter_gap,
            #[cfg(feature = "diff-view")]
            split_wrap_pane_widths: if let (Some(sync), Some(side)) =
                (&text_area.split_wrap_sync, text_area.split_wrap_side)
            {
                split_wrap_pane_widths(sync, side)
            } else {
                None
            },
            #[cfg(feature = "diff-view")]
            split_wrap_scrollbar_cols: text_area
                .split_wrap_sync
                .as_ref()
                .map(split_wrap_scrollbar_cols_pair),
            #[cfg(feature = "diff-view")]
            split_wrap_layout_pass: text_area
                .split_wrap_sync
                .as_ref()
                .map(split_wrap_layout_pass)
                .unwrap_or(0),
        },
    );

    if let Some(cache) = cache.as_ref()
        && let Some(entry) = cache.entries.iter().find(|e| e.key == cache_key)
    {
        let mut geometry = entry.geometry.clone();
        geometry.cursor_visual_line = if text_area.wrap {
            cursor_visual_line_for_wrapped_lines(&entry.lines, text_area.cursor)
        } else {
            cursor_visual_line_for_cursor(&entry.lines, text_area.cursor)
        };

        #[cfg(feature = "diff-view")]
        if cache_key.split_wrap_layout_pass == 1
            && let (Some(sync), Some(side)) =
                (&text_area.split_wrap_sync, text_area.split_wrap_side)
            && let Some(heights) = &entry.own_source_heights
        {
            record_pass1_source_heights(sync, side, heights);
        }
        return geometry;
    }

    let logical_lines_count = logical_line_count(value);

    let gutter_width = text_area_total_gutter_width(
        logical_lines_count,
        line_numbers,
        min_line_number_width,
        text_area.gutter_col_width,
        text_area.gutter_gap,
    ) as usize;

    let scrollbar_cols: u16 = if cache_key.reserve_scrollbar_col {
        1u16.saturating_add(cache_key.reserve_scrollbar_gap)
    } else {
        0
    };

    let reserve_cursor = text_area_cursor_reserve(text_area.wrap, text_area.read_only);
    let content_width = inner_w
        .saturating_sub(gutter_width as u16)
        .saturating_sub(scrollbar_cols)
        .saturating_sub(reserve_cursor) as usize;

    if content_width == 0 {
        return TextAreaGeometry {
            inner_w: 0,
            inner_h: 0,
            gutter_width: 0,
            content_width: 0,
            total_visual_lines: 1,
            cursor_visual_line: 0,
            max_line_width: 0,
            viewport_height: 0,
            h_scrollbar_visible: false,
            v_scrollbar_visible: false,
            scrollbar_zones: Vec::new(),
        };
    }

    let mut current_byte_offset = 0usize;
    let mut line_starts: Vec<usize> = Vec::with_capacity(logical_lines_count);
    let mut line_end_offsets: Vec<usize> = Vec::with_capacity(logical_lines_count);
    let mut visual_lines: Vec<TextAreaVisualLine> = Vec::new();
    let lines = if value.is_empty() {
        vec![""]
    } else {
        value.split('\n').collect()
    };
    for line in &lines {
        line_starts.push(current_byte_offset);
        line_end_offsets.push(current_byte_offset + line.len());
        current_byte_offset += line.len() + 1;
    }

    let has_inline_virtual_text = text_area.virtual_texts.iter().any(|vt| {
        vt.placement == super::VirtualTextPlacement::Inline
            && super::virtual_text_content_width(vt) > 0
    });

    let max_line_width = if has_inline_virtual_text {
        let mut max_line_width = 0usize;
        for (idx, line) in lines.iter().enumerate() {
            let line_start = line_starts.get(idx).copied().unwrap_or(0);
            let line_end = line_end_offsets.get(idx).copied().unwrap_or(line_start);
            let insertions = super::inline_virtual_insertions_for_line(
                value,
                &text_area.virtual_texts,
                line_start,
                line_end,
            );
            let width = layout_line_with_inline_virtual_text(
                line,
                VirtualTextLayoutCtx {
                    line_start_abs: line_start,
                    line_num: idx + 1,
                    wrap,
                    content_width,
                    caret: (!text_area.read_only).then_some(cursor),
                    sentinel: sentinel.as_ref(),
                    tab_stop: text_area.tab_display_width as usize,
                    insertions: &insertions,
                },
                &mut visual_lines,
            );
            max_line_width = max_line_width.max(width);
        }
        max_line_width
    } else {
        let prepared_key = TextAreaPreparedKey {
            value_hash,
            sentinel_ph_width,
            sentinel_count,
            custom_sentinel_hash: custom_sentinel_widths_hash,
            tab_stop: text_area.tab_display_width,
        };
        let prepared = if let Some(cache) = cache.as_deref_mut() {
            cache.prepared_text(
                prepared_key,
                value,
                sentinel.as_ref(),
                text_area.tab_display_width,
            )
        } else {
            Arc::new(prepare_text(
                value,
                sentinel.as_ref(),
                text_area.tab_display_width as usize,
            ))
        };

        let max_line_width = max_logical_line_width(&prepared);
        if wrap {
            let line_ranges = if text_area.read_only {
                layout_lines(&prepared, content_width)
            } else {
                layout_lines_with_caret(&prepared, content_width, cursor)
            };
            let mut logical_line_idx = 0usize;
            let mut seen_visual_in_line = vec![false; logical_lines_count.max(1)];
            for range in line_ranges {
                while logical_line_idx + 1 < logical_lines_count
                    && range.start >= line_starts[logical_line_idx + 1]
                {
                    logical_line_idx += 1;
                }
                let line_num = logical_line_idx + 1;
                let continuation = seen_visual_in_line[logical_line_idx];
                seen_visual_in_line[logical_line_idx] = true;
                let logical_line_start = line_starts[logical_line_idx];
                let start_col = str_visual_width_with_tabs(
                    &value[logical_line_start..range.start],
                    sentinel.as_ref(),
                    0,
                    text_area.tab_display_width as usize,
                );
                let end_col = start_col.saturating_add(str_visual_width_with_tabs(
                    &value[range.start..range.end],
                    sentinel.as_ref(),
                    start_col,
                    text_area.tab_display_width as usize,
                ));
                visual_lines.push(TextAreaVisualLine {
                    line_num,
                    continuation,
                    start: range.start,
                    end: range.end,
                    visual_start_col: start_col,
                    visual_end_col: end_col,
                    starts_with_virtual_text: false,
                    ends_with_virtual_text: false,
                });
            }
        } else {
            for (idx, (&start, &end)) in line_starts.iter().zip(line_end_offsets.iter()).enumerate()
            {
                let line_width = str_visual_width_with_tabs(
                    &value[start..end],
                    sentinel.as_ref(),
                    0,
                    text_area.tab_display_width as usize,
                );
                visual_lines.push(TextAreaVisualLine {
                    line_num: idx + 1,
                    continuation: false,
                    start,
                    end,
                    visual_start_col: 0,
                    visual_end_col: line_width,
                    starts_with_virtual_text: false,
                    ends_with_virtual_text: false,
                });
            }
        }
        max_line_width
    };

    // allow(unused_mut): the diff-view feature gate below conditionally mutates these.
    #[allow(unused_mut)]
    let mut total_visual_lines = visual_lines.len();
    #[allow(unused_mut)]
    let mut cursor_visual_line = if wrap {
        cursor_visual_line_for_wrapped_lines(&visual_lines, cursor)
    } else {
        cursor_visual_line_for_cursor(&visual_lines, cursor)
    };

    #[cfg(feature = "diff-view")]
    if text_area.wrap
        && content_width > 0
        && let Some(peer_lines) = text_area.peer_source_lines.as_deref()
    {
        let mut own_source_heights = vec![0u16; logical_lines_count];
        for vline in &visual_lines {
            let idx = vline.line_num.saturating_sub(1);
            if idx < own_source_heights.len() {
                own_source_heights[idx] = own_source_heights[idx].saturating_add(1);
            }
        }

        let sync_side = (&text_area.split_wrap_sync, text_area.split_wrap_side);

        let padding = if let (Some(sync), Some(side)) = sync_side {
            let pass = split_wrap_layout_pass(sync);
            if pass == 1 {
                record_pass1_source_heights(sync, side, &own_source_heights);
                vec![]
            } else if pass == 2 {
                if let Some(peer_h) = peer_pass1_source_heights(sync, side) {
                    compute_split_wrap_padding_from_heights(&own_source_heights, peer_h.as_ref())
                } else {
                    let peer_sim_w = peer_simulated_content_width(sync, side, content_width as u16)
                        .unwrap_or(content_width as u16);
                    compute_split_wrap_padding(&own_source_heights, peer_lines, peer_sim_w)
                }
            } else {
                let peer_sim_w = peer_simulated_content_width(sync, side, content_width as u16)
                    .unwrap_or(content_width as u16);
                compute_split_wrap_padding(&own_source_heights, peer_lines, peer_sim_w)
            }
        } else {
            compute_split_wrap_padding(&own_source_heights, peer_lines, content_width as u16)
        };

        if padding.iter().any(|&extra| extra > 0) {
            let extra_rows = padding
                .iter()
                .map(|&v| usize::from(v))
                .fold(0usize, usize::saturating_add);
            let mut padded = Vec::with_capacity(visual_lines.len().saturating_add(extra_rows));
            let mut src = 0usize;

            for source_idx in 0..logical_lines_count {
                let line_num = source_idx + 1;
                while src < visual_lines.len() && visual_lines[src].line_num == line_num {
                    padded.push(visual_lines[src].clone());
                    src = src.saturating_add(1);
                }

                let line_end = line_end_offsets.get(source_idx).copied().unwrap_or(0);
                let extra = padding.get(source_idx).copied().unwrap_or(0);
                for _ in 0..extra {
                    padded.push(TextAreaVisualLine {
                        line_num,
                        continuation: true,
                        start: line_end,
                        end: line_end,
                        visual_start_col: 0,
                        visual_end_col: 0,
                        starts_with_virtual_text: false,
                        ends_with_virtual_text: false,
                    });
                }
            }

            if src < visual_lines.len() {
                padded.extend_from_slice(&visual_lines[src..]);
            }

            visual_lines = padded;
            total_visual_lines = visual_lines.len();
            cursor_visual_line = cursor_visual_line_for_cursor(&visual_lines, cursor);
        }
    }

    let geometry = TextAreaGeometry {
        inner_w,
        gutter_width,
        content_width,
        total_visual_lines: total_visual_lines.max(1),
        cursor_visual_line,
        max_line_width,
        ..TextAreaGeometry::default()
    };

    #[cfg(feature = "diff-view")]
    let mut own_source_heights_cache: Option<Arc<Vec<u16>>> = None;
    #[cfg(feature = "diff-view")]
    if text_area.wrap && content_width > 0 && text_area.peer_source_lines.is_some() {
        let mut own_source_heights = vec![0u16; logical_lines_count];
        for vline in &visual_lines {
            let idx = vline.line_num.saturating_sub(1);
            if idx < own_source_heights.len() {
                own_source_heights[idx] = own_source_heights[idx].saturating_add(1);
            }
        }
        own_source_heights_cache = Some(Arc::new(own_source_heights));
    }

    if let Some(cache) = cache {
        cache.insert_with_lines(
            cache_key,
            geometry.clone(),
            visual_lines,
            #[cfg(feature = "diff-view")]
            own_source_heights_cache,
        );
    }

    geometry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::ImageContent;
    use crate::core::element::Element;
    use crate::core::node::{NodeKind, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::{Rect, Span};
    use crate::widgets::text_area::IMAGE_SENTINEL_BASE;

    /// Helper: build a minimal TextArea with specific fields for testing.
    /// Disables scrollbar so wrapped content can use the full inner width.
    fn make_textarea(value: &str, wrap: bool, line_numbers: bool) -> TextArea {
        TextArea::new(value)
            .wrap(wrap)
            .line_numbers(line_numbers)
            .scrollbar(false)
            .border(false)
    }

    /// Call calculate_text_area_visual_metrics with no cache.
    fn calc(ta: &TextArea, inner_w: u16) -> (usize, usize, usize, usize) {
        let h = hash_text(ta.value.as_ref());
        let g = calculate_text_area_visual_metrics(ta, inner_w, false, h, None);
        (
            g.total_visual_lines,
            g.cursor_visual_line,
            g.max_line_width,
            g.content_width,
        )
    }

    // ---------------------------------------------------------------
    // 1. Single-line text, no wrapping needed
    // ---------------------------------------------------------------
    #[test]
    fn single_line_no_wrap() {
        // "hello" is 5 cols wide. With inner_w=20 and no scrollbar,
        // content_width uses the full 20 columns.
        // Fits in one visual line.
        let ta = make_textarea("hello", true, false);
        let (total, cursor_vl, max_w, content_w) = calc(&ta, 20);

        assert_eq!(total, 1, "single short line => 1 visual line");
        assert_eq!(cursor_vl, 0, "cursor at 0 => visual line 0");
        assert_eq!(max_w, 5, "max line width = visual width of 'hello'");
        assert_eq!(content_w, 20, "wrapped content uses the full inner width");
    }

    // ---------------------------------------------------------------
    // 2. Wrapping at exact boundary width
    // ---------------------------------------------------------------
    #[test]
    fn wrap_at_exact_boundary() {
        // content_width = 5. Text "abcdefgh" is 8 chars wide.
        // With no whitespace break points, wraps by character:
        //   visual line 0: "abcde" (5 cols)
        //   visual line 1: "fgh" (3 cols)
        let ta = make_textarea("abcdefgh", true, false);
        let (total, _cursor_vl, max_w, content_w) = calc(&ta, 5);

        assert_eq!(content_w, 5);
        assert_eq!(total, 2, "8 chars in 5-wide => 2 visual lines");
        assert_eq!(max_w, 8, "max_line_width reflects the full logical line");
    }

    #[test]
    fn full_width_editable_line_reflows_only_caret_row() {
        let editable = make_textarea("abcde", true, false).cursor(5);
        let read_only = editable.clone().read_only(true);
        let mut cache = TextAreaVisualCache::default();

        let editable_geometry = calculate_text_area_visual_metrics(
            &editable,
            5,
            false,
            hash_text(editable.value.as_ref()),
            Some(&mut cache),
        );
        let (read_only_total, _, _, read_only_content_w) = calc(&read_only, 5);

        assert_eq!(editable_geometry.content_width, 5);
        assert_eq!(read_only_content_w, editable_geometry.content_width);
        assert_eq!(editable_geometry.total_visual_lines, 2);
        assert_eq!(editable_geometry.cursor_visual_line, 1);
        assert_eq!(
            cache
                .latest_lines()
                .expect("editable layout is cached")
                .iter()
                .map(|line| (line.start, line.end))
                .collect::<Vec<_>>(),
            vec![(0, 4), (4, 5)]
        );
        assert_eq!(read_only_total, 1);
    }

    #[test]
    fn caret_reservation_reflows_at_word_boundary() {
        let value = "really long tex";
        let ta = make_textarea(value, true, false).cursor(value.len());
        let mut cache = TextAreaVisualCache::default();

        let geometry =
            calculate_text_area_visual_metrics(&ta, 15, false, hash_text(value), Some(&mut cache));

        assert_eq!(geometry.total_visual_lines, 2);
        assert_eq!(geometry.cursor_visual_line, 1);
        assert_eq!(
            cache
                .latest_lines()
                .expect("layout is cached")
                .iter()
                .map(|line| (line.start, line.end))
                .collect::<Vec<_>>(),
            vec![(0, 12), (12, 15)]
        );
    }

    #[test]
    fn caret_reservation_does_not_shrink_other_wrapped_rows() {
        let value = "abcdefghijklmno\nreally long tex";
        let ta = make_textarea(value, true, false).cursor(value.len());
        let mut cache = TextAreaVisualCache::default();

        calculate_text_area_visual_metrics(&ta, 15, false, hash_text(value), Some(&mut cache));

        assert_eq!(
            cache
                .latest_lines()
                .expect("layout is cached")
                .iter()
                .map(|line| (line.start, line.end))
                .collect::<Vec<_>>(),
            vec![(0, 15), (16, 28), (28, 31)]
        );
    }

    #[test]
    fn auto_height_tracks_cursor_dependent_caret_row() {
        let value = "really long tex";
        let at_start = make_textarea(value, true, false)
            .width(crate::style::Length::Px(15))
            .height(crate::style::Length::Auto);
        let at_end = at_start.clone().cursor(value.len());

        assert_eq!(measure_text_area_constrained(&at_start, Some(15)).1, 1);
        assert_eq!(measure_text_area_constrained(&at_end, Some(15)).1, 2);
    }

    #[test]
    fn cursor_change_invalidates_caret_dependent_visual_layout() {
        let value = "really long tex";
        let at_start = make_textarea(value, true, false);
        let at_end = at_start.clone().cursor(value.len());
        let mut cache = TextAreaVisualCache::default();

        let start_geometry = calculate_text_area_visual_metrics(
            &at_start,
            15,
            false,
            hash_text(value),
            Some(&mut cache),
        );
        let end_geometry = calculate_text_area_visual_metrics(
            &at_end,
            15,
            false,
            hash_text(value),
            Some(&mut cache),
        );

        assert_eq!(start_geometry.total_visual_lines, 1);
        assert_eq!(end_geometry.total_visual_lines, 2);
        assert_eq!(cache.entries.len(), 2);
    }

    #[test]
    fn overflowing_internal_separator_reuses_previous_word_break() {
        let value = "really long tex forgot \"t\"";
        let ta = make_textarea(value, true, false).cursor(value.len());
        let mut cache = TextAreaVisualCache::default();

        calculate_text_area_visual_metrics(&ta, 15, false, hash_text(value), Some(&mut cache));

        assert_eq!(
            cache
                .latest_lines()
                .expect("layout is cached")
                .iter()
                .map(|line| (line.start, line.end))
                .collect::<Vec<_>>(),
            vec![(0, 12), (12, value.len())]
        );
    }

    #[test]
    fn trailing_space_and_next_input_stay_on_caret_row() {
        for (value, expected_col) in [("really long tex ", 4), ("really long tex d", 5)] {
            let text_area = make_textarea(value, true, false).cursor(value.len());
            let mut cache = TextAreaVisualCache::default();
            let geometry = calculate_text_area_visual_metrics(
                &text_area,
                15,
                false,
                hash_text(value),
                Some(&mut cache),
            );
            let lines = cache.latest_lines().expect("layout is cached");

            assert_eq!(
                lines
                    .iter()
                    .map(|line| (line.start, line.end))
                    .collect::<Vec<_>>(),
                vec![(0, 12), (12, value.len())]
            );
            assert_eq!(geometry.cursor_visual_line, 1);
            assert_eq!(
                str_visual_width_with_tabs(
                    &value[lines[1].start..value.len()],
                    None,
                    lines[1].visual_start_col,
                    4,
                ),
                expected_col
            );
        }
    }

    #[test]
    fn exact_width_trailing_space_keeps_full_row_before_next_input() {
        for (value, expected_ranges, expected_col) in [
            ("really long te ", vec![(0, 15), (15, 15)], 0),
            ("really long te t", vec![(0, 15), (15, 16)], 1),
        ] {
            let text_area = make_textarea(value, true, false).cursor(value.len());
            let mut cache = TextAreaVisualCache::default();
            let geometry = calculate_text_area_visual_metrics(
                &text_area,
                15,
                false,
                hash_text(value),
                Some(&mut cache),
            );
            let lines = cache.latest_lines().expect("layout is cached");

            assert_eq!(
                lines
                    .iter()
                    .map(|line| (line.start, line.end))
                    .collect::<Vec<_>>(),
                expected_ranges
            );
            assert_eq!(geometry.cursor_visual_line, 1);
            assert_eq!(
                str_visual_width_with_tabs(
                    &value[lines[1].start..value.len()],
                    None,
                    lines[1].visual_start_col,
                    4,
                ),
                expected_col
            );
        }
    }

    #[test]
    fn cursor_on_wrap_boundary_tracks_continuation_visual_line_start() {
        let ta = make_textarea("abcdefgh", true, false).cursor(5);
        let (total, cursor_vl, _, content_w) = calc(&ta, 5);

        assert_eq!(content_w, 5);
        assert_eq!(total, 2);
        assert_eq!(
            cursor_vl, 1,
            "cursor at wrapped boundary belongs to the continuation row start"
        );
    }

    // ---------------------------------------------------------------
    // 3. Wrapping with CJK (double-width) characters
    // ---------------------------------------------------------------
    #[test]
    fn wrap_with_cjk_characters() {
        // Each CJK char is 2 columns wide.
        // "你好世界" = 4 chars × 2 cols = 8 visual cols.
        // content_width = 5. Each CJK still fits exactly 2 per row.
        //   visual line 0: "你好" (4 cols)
        //   visual line 1: "世界" (4 cols)
        let ta = make_textarea("你好世界", true, false);
        let (total, _, max_w, content_w) = calc(&ta, 5);

        assert_eq!(content_w, 5);
        assert_eq!(max_w, 8);
        assert_eq!(total, 2, "4 CJK chars (8 cols) in 5-wide => 2 visual lines");

        // With content_width = 6, three CJK characters fit on the first row.
        let ta2 = make_textarea("你好世界", true, false);
        let (total2, _, _, content_w2) = calc(&ta2, 6);
        assert_eq!(content_w2, 6);
        assert_eq!(
            total2, 2,
            "CJK won't split mid-char; still 2 lines at width 6"
        );
    }

    // ---------------------------------------------------------------
    // 4. Line number gutter width calculation
    // ---------------------------------------------------------------
    #[test]
    fn line_number_gutter_width() {
        // With line_numbers=true and no scrollbar, gutter eats into content_width.
        // For 1 line: digits=1, gutter_width = 1 + 2 = 3.
        // content_width = inner_w - gutter(3).
        let ta = make_textarea("hello", true, true);
        let (_, _, _, content_w) = calc(&ta, 20);
        // 1 logical line => digits = max("1".len(), 0) = 1, gutter = 3
        assert_eq!(content_w, 20 - 3, "gutter(3) subtracted");

        // 100 lines => digits = 3 ("100"), gutter = 5.
        let many_lines: String = (1..=100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let ta2 = make_textarea(&many_lines, false, true);
        let (_, _, _, content_w2) = calc(&ta2, 30);
        // digits = "100".len() = 3, gutter = 5
        assert_eq!(
            content_w2,
            30 - 5 - 1,
            "100 lines => 3-digit gutter (width 5)"
        );

        // min_line_number_width forces minimum digit count
        let ta3 = make_textarea("x", true, true).min_line_number_width(4);
        let (_, _, _, content_w3) = calc(&ta3, 20);
        // 1 line => digits = max(1, 4) = 4, gutter = 6
        assert_eq!(content_w3, 20 - 6, "min_line_number_width=4 => gutter=6");
    }

    #[test]
    fn gutter_gap_reduces_content_width() {
        let ta = make_textarea("hello", true, true).gutter_inset(1);
        let (_, _, _, content_w) = calc(&ta, 20);

        assert_eq!(content_w, 20 - 4, "gutter(3) + gap(1)");
    }

    // ---------------------------------------------------------------
    // 5. Empty text and single character edge cases
    // ---------------------------------------------------------------
    #[test]
    fn empty_and_single_char() {
        // Empty text always produces exactly 1 visual line with 0 max width.
        let ta_empty = make_textarea("", true, false);
        let (total, cursor_vl, max_w, _) = calc(&ta_empty, 20);
        assert_eq!(total, 1, "empty text => 1 visual line");
        assert_eq!(cursor_vl, 0);
        assert_eq!(max_w, 0);

        // Single character "x" => 1 visual line, width 1.
        let ta_one = make_textarea("x", true, false);
        let (total, _, max_w, _) = calc(&ta_one, 20);
        assert_eq!(total, 1);
        assert_eq!(max_w, 1);

        // At inner_w=1 each character occupies one row. The caret remains at
        // its configured start position, so no final continuation is needed.
        let ta_narrow = make_textarea("hello", true, false);
        let result = calc(&ta_narrow, 1);
        assert_eq!(result, (5, 0, 5, 1));
    }

    #[test]
    fn measure_constrained_percent_width_uses_available_space() {
        let ta = make_textarea("hello", false, false).width(crate::style::Length::Percent(50));

        let (w, h) = measure_text_area_constrained(&ta, Some(40));

        assert_eq!(w, 20);
        assert_eq!(h, 1);
    }

    #[test]
    fn auto_height_counts_standalone_horizontal_scrollbar_row() {
        let ta = make_textarea("123456789\nabc", false, false)
            .width(crate::style::Length::Px(5))
            .height(crate::style::Length::Auto)
            .h_scrollbar(true);

        let (w, h) = measure_text_area_constrained(&ta, Some(5));

        assert_eq!(w, 5);
        assert_eq!(h, 3);
    }

    #[test]
    fn measure_includes_gutter_gap() {
        let ta = make_textarea("x", false, true)
            .gutter_inset(1)
            .border(false);

        let (w, h) = measure_text_area(&ta);

        assert_eq!(w, 6);
        assert_eq!(h, 1);
    }

    #[test]
    fn sentinel_wrapping_uses_placeholder_width() {
        let value = format!("a{}b", IMAGE_SENTINEL_BASE);
        let ta = make_textarea(&value, true, false).images(vec![ImageContent {
            data: String::new(),
            mime: "image/png",
            filename: None,
        }]);

        let (total, _, max_w, content_w) = calc(&ta, 5);

        assert_eq!(content_w, 5);
        assert_eq!(max_w, 9);
        assert_eq!(
            total, 3,
            "image sentinel placeholder width participates in wrapping"
        );
    }

    #[test]
    fn inline_virtual_text_participates_in_wrapping_without_widening_measurement() {
        let plain = make_textarea("ab", true, false);
        let hinted = make_textarea("ab", true, false).virtual_text(
            super::super::TextAreaVirtualText::inline(1, vec![Span::new("<<<")]),
        );

        assert_eq!(measure_text_area(&plain), measure_text_area(&hinted));

        let hash = hash_text(hinted.value.as_ref());
        let mut cache = TextAreaVisualCache::default();
        let geometry =
            calculate_text_area_visual_metrics(&hinted, 4, false, hash, Some(&mut cache));
        let lines = cache.latest_lines().expect("visual lines should be cached");

        assert_eq!(geometry.content_width, 4);
        assert_eq!(geometry.max_line_width, 5);
        assert_eq!(geometry.total_visual_lines, 2);
        assert_eq!(lines[0].start, 0);
        assert_eq!(lines[0].end, 1);
        assert!(lines[0].ends_with_virtual_text);
        assert_eq!(lines[1].start, 1);
        assert_eq!(lines[1].end, 2);
    }

    #[test]
    fn virtual_text_hash_invalidates_visual_cache() {
        let short = make_textarea("ab", true, false).cursor(2).virtual_text(
            super::super::TextAreaVirtualText::inline(1, vec![Span::new("<")]),
        );
        let long = make_textarea("ab", true, false).cursor(2).virtual_text(
            super::super::TextAreaVirtualText::inline(1, vec![Span::new("<<<")]),
        );
        let hash = hash_text(short.value.as_ref());
        let mut cache = TextAreaVisualCache::default();

        let short_geo =
            calculate_text_area_visual_metrics(&short, 5, false, hash, Some(&mut cache));
        let long_geo = calculate_text_area_visual_metrics(&long, 5, false, hash, Some(&mut cache));

        assert_eq!(short_geo.total_visual_lines, 1);
        assert_eq!(long_geo.total_visual_lines, 2);
        assert_eq!(cache.entries.len(), 2);
    }

    #[test]
    fn tab_stop_invalidates_visual_cache() {
        let narrow_tab = make_textarea("a\tb", true, false).tab_display_width(2);
        let wide_tab = make_textarea("a\tb", true, false).tab_display_width(8);
        let hash = hash_text(narrow_tab.value.as_ref());
        let mut cache = TextAreaVisualCache::default();

        let narrow_geo =
            calculate_text_area_visual_metrics(&narrow_tab, 5, false, hash, Some(&mut cache));
        let wide_geo =
            calculate_text_area_visual_metrics(&wide_tab, 5, false, hash, Some(&mut cache));

        assert_eq!(narrow_geo.total_visual_lines, 1);
        assert!(wide_geo.total_visual_lines > 1);
        assert_eq!(cache.entries.len(), 2);
    }

    #[test]
    fn cursor_metrics_keep_buffer_position_but_rect_uses_inline_virtual_width() {
        let root: Element = make_textarea("ab", false, false)
            .width(crate::style::Length::Px(10))
            .height(crate::style::Length::Px(1))
            .cursor(2)
            .virtual_text(super::super::TextAreaVirtualText::inline(
                1,
                vec![Span::new("xxx")],
            ))
            .into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 1,
            },
            None,
        );

        let node = tree.node(tree.root);
        let NodeKind::TextArea(text_area) = &node.kind else {
            panic!("expected TextArea root");
        };
        let metrics = text_area.metrics(node.rect);
        let cursor = metrics.cursor.expect("cursor should be visible");

        assert_eq!(cursor.byte_offset, 2);
        assert_eq!(cursor.position.column, 2);
        assert_eq!(cursor.rect.x, 5);
    }

    #[test]
    fn prepared_text_cache_reused_across_width_changes() {
        let ta = make_textarea("hello world", true, false);
        let hash = hash_text(ta.value.as_ref());
        let mut cache = TextAreaVisualCache::default();

        let _ = calculate_text_area_visual_metrics(&ta, 10, false, hash, Some(&mut cache));
        assert_eq!(cache.prepared_entries.len(), 1);
        let first_prepared = cache.prepared_entries[0].prepared.clone();

        let _ = calculate_text_area_visual_metrics(&ta, 16, false, hash, Some(&mut cache));
        assert_eq!(cache.prepared_entries.len(), 1);
        assert!(Arc::ptr_eq(
            &first_prepared,
            &cache.prepared_entries[0].prepared
        ));
        assert_eq!(
            cache.entries.len(),
            2,
            "visual layout cache keys still vary by width"
        );
    }
}
