//! Internal render-tree node for [`DocumentView`](super::DocumentView).

use std::rc::Rc;
use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::node::{
    NodeKind, ScrollbarZone, ScrollbarZonesParams, WidgetNode, compute_scrollbar_zones,
};
use crate::style::{
    BorderStyle, Padding, Rect, ScrollbarVariant, Span, Style, StyleSlot, Theme, ThemeRole,
};

/// Pre-computed content-area layout derived from a [`DocumentViewNode`] and its
/// `inner` rect.  This eliminates ~180 lines of duplicated gutter / scrollbar /
/// content-rect arithmetic that was spread across input, scrollbar, and event
/// modules.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ContentLayout {
    /// Whether the horizontal scrollbar overlaps the border (Integrated style).
    pub h_scrollbar_over_border: bool,
    /// X origin of the content area (after gutter).
    pub content_x: i16,
    /// Y origin of the content area (same as `inner.y`).
    pub content_y: i16,
    /// Width of the text content area (inner minus gutter minus scrollbar).
    pub content_width: u16,
    /// Height of the text content area (inner minus h-scrollbar row if needed).
    pub content_height: u16,
}
use crate::widgets::scroll::SmoothScrollState;
use crate::widgets::scroll_view::{ScrollBehavior, ScrollEvent};
use crate::widgets::table::{
    TableBorderLineKind, distribute_extra_width, shrink_widths_to_fit, table_border_glyphs,
    table_border_line, table_fixed_chars, table_render_width,
};

use super::format::FormatCache;
use super::{
    DocumentClickEvent, DocumentLineNumberMode, DocumentSelectEvent, DocumentStyles,
    DocumentTableWidthMode, DocumentView, FormattedDocument, FormattedLink,
};

/// The visual-line kind after block flattening.
#[derive(Clone, Debug)]
pub(crate) enum VisualLineKind {
    /// A normal text line (possibly word-wrapped).
    Text {
        spans: Vec<Span>,
        indent_cols: u16,
        continuation: bool,
        links: Vec<FormattedLink>,
    },
    /// A table row rendered with box-drawing.
    TableRow {
        cells: Vec<Vec<Span>>,
        /// Plain text for the currently rendered visual sub-line of each cell.
        cell_line_texts: Vec<Arc<str>>,
        /// Full, unwrapped plain text for each logical cell in the row.
        full_cell_texts: Vec<Arc<str>>,
        alignments: Vec<super::ColumnAlign>,
        /// Total per-column width including horizontal cell padding.
        widths: Vec<u16>,
        table_id: usize,
        row_index: usize,
        row_line_index: usize,
        border_variant: BorderStyle,
        outer_frame: bool,
        column_separators: bool,
        cell_padding: u16,
    },
    /// Table border line (top/mid/bottom).
    TableBorder {
        kind: TableBorderKind,
        widths: Vec<u16>,
        border_variant: BorderStyle,
        outer_frame: bool,
        column_separators: bool,
    },
    /// A horizontal rule.
    HorizontalRule,
    /// A code line (from a code block, possibly syntax-highlighted).
    CodeLine {
        spans: Vec<Span>,
        block_style: Style,
    },
    /// A rendered row from a parsed Mermaid diagram.
    DiagramRow {
        spans: Vec<Span>,
        /// Original Mermaid source. Each visual row maps to the same source span so
        /// row-granular hit testing can select the whole diagram; copy extraction
        /// de-duplicates continuation rows.
        source_text: Option<Arc<str>>,
    },
    /// A blockquote line (with left-bar decoration).
    BlockQuoteLine {
        spans: Vec<Span>,
        depth: u16,
        links: Vec<FormattedLink>,
    },
}

/// Position within a table border row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TableBorderKind {
    /// ┌──┬──┐
    Top,
    /// ├──┼──┤
    Mid,
    /// └──┴──┘
    Bottom,
}

/// Rectangular table-cell selection for `DocumentView`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DocumentTableRectSelection {
    pub table_id: usize,
    pub row_start: usize,
    pub row_end: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub anchor_row_index: usize,
    pub anchor_col_index: usize,
    pub anchor_row_line_index: usize,
    pub anchor_cell_line_anchor_byte: usize,
    pub cursor_row_index: usize,
    pub cursor_col_index: usize,
    pub cursor_row_line_index: usize,
    pub cursor_cell_line_anchor_byte: usize,
    pub tsv_text: Arc<str>,
}

/// A single visual line after block flattening.
#[derive(Clone, Debug)]
pub(crate) struct DocumentVisualLine {
    pub kind: VisualLineKind,
    pub source_line: usize,
}

pub(crate) type VisualCacheKey = (u64, u64, u64, u16, bool, u64, u64);

/// One cached flattened visual-line result.
#[derive(Clone, Debug)]
pub(crate) struct VisualCacheEntry {
    pub key: VisualCacheKey,
    pub lines: Vec<DocumentVisualLine>,
    pub source_line_map: Vec<usize>,
    pub line_texts: Vec<Arc<str>>,
    pub line_starts: Vec<usize>,
    pub line_lengths: Vec<usize>,
    pub flat_text: Arc<str>,
    pub max_line_width: u16,
}

/// Cache for flattened visual lines (avoids re-flattening on every frame).
#[derive(Clone, Debug, Default)]
pub(crate) struct VisualCache {
    /// Cache key: (content_hash, formatter_hash, syntax_hash, inner_width, wrap, table_config_hash, peer_hash).
    pub key: Option<VisualCacheKey>,
    /// Flattened visual lines.
    pub lines: Vec<DocumentVisualLine>,
    /// Source-line mapping: visual_line_idx → source_line.
    pub source_line_map: Vec<usize>,
    /// Plain rendered text for each visual line (content area, no gutter).
    pub line_texts: Vec<Arc<str>>,
    /// Byte start offset in `flat_text` for each visual line.
    pub line_starts: Vec<usize>,
    /// Byte length for each visual line.
    pub line_lengths: Vec<usize>,
    /// Flattened text buffer of all visual lines joined by `\n`.
    pub flat_text: Arc<str>,
    /// Maximum visual line width (for horizontal scrollbar).
    pub max_line_width: u16,
    /// Previous visual result. Split-wrap diff panes alternate pass 1 and pass 2
    /// keys during a single reconcile, so one slot would evict every frame.
    previous: Option<Box<VisualCacheEntry>>,
}

impl VisualCache {
    fn take_active_entry(&mut self) -> Option<VisualCacheEntry> {
        Some(VisualCacheEntry {
            key: self.key.take()?,
            lines: std::mem::take(&mut self.lines),
            source_line_map: std::mem::take(&mut self.source_line_map),
            line_texts: std::mem::take(&mut self.line_texts),
            line_starts: std::mem::take(&mut self.line_starts),
            line_lengths: std::mem::take(&mut self.line_lengths),
            flat_text: std::mem::take(&mut self.flat_text),
            max_line_width: self.max_line_width,
        })
    }

    pub(crate) fn promote(&mut self, key: &VisualCacheKey) -> bool {
        if self.key.as_ref() == Some(key) {
            return true;
        }

        let Some(mut previous) = self.previous.take() else {
            return false;
        };
        if previous.key != *key {
            self.previous = Some(previous);
            return false;
        }

        let active = self.take_active_entry().map(Box::new);
        self.key = Some(previous.key);
        self.lines = std::mem::take(&mut previous.lines);
        self.source_line_map = std::mem::take(&mut previous.source_line_map);
        self.line_texts = std::mem::take(&mut previous.line_texts);
        self.line_starts = std::mem::take(&mut previous.line_starts);
        self.line_lengths = std::mem::take(&mut previous.line_lengths);
        self.flat_text = std::mem::take(&mut previous.flat_text);
        self.max_line_width = previous.max_line_width;
        self.previous = active;
        true
    }

    pub(crate) fn preserve_active_for_insert(&mut self, incoming_key: &VisualCacheKey) {
        if self.key.as_ref().is_none_or(|key| key == incoming_key) {
            return;
        }
        self.previous = self.take_active_entry().map(Box::new);
    }
}

/// The internal render node for `DocumentView`.
#[derive(Clone)]
pub(crate) struct DocumentViewNode {
    // ── Content ──────────────────────────────────────────────────────────
    pub value: Arc<str>,
    pub content_type: Option<Arc<str>>,
    pub formatter: Option<Rc<dyn super::ContentFormatter>>,

    // ── Layout ───────────────────────────────────────────────────────────
    pub wrap: bool,
    pub line_numbers: bool,
    pub min_line_number_width: u8,
    pub line_number_separator: bool,
    pub line_number_content_gap: u16,
    pub line_number_mode: DocumentLineNumberMode,
    pub highlight_full_width: bool,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
    pub table_wrap: bool,
    pub table_width_mode: DocumentTableWidthMode,
    pub table_outer_frame: bool,
    pub table_column_separators: bool,
    pub table_row_separators: super::TableRowSeparators,
    pub table_cell_padding: u16,
    pub table_border_variant: BorderStyle,

    // ── Styling ──────────────────────────────────────────────────────────
    pub style: Style,
    pub line_number_style: Style,
    pub hover_style: StyleSlot,
    pub focus_style: StyleSlot,
    pub focus_content_style: Style,
    pub selection_style: StyleSlot,
    pub doc_styles: DocumentStyles,
    pub hover_border_style: Option<BorderStyle>,

    // ── Scrolling ────────────────────────────────────────────────────────
    pub scroll_offset: usize,
    pub scroll_override: Option<usize>,
    pub scroll_to_source_line: Option<usize>,
    pub cancelled_scroll_to_source_line: Option<usize>,
    pub scroll_behavior: ScrollBehavior,
    pub smooth_scroll: SmoothScrollState,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub scrollbar_thumb_style: Option<Style>,
    pub scrollbar_thumb_focus_style: Option<Style>,
    pub scrollbar_track_style: Option<Style>,
    pub scrollbar_thumb: Option<char>,
    pub h_scrollbar: bool,
    pub h_scrollbar_variant: ScrollbarVariant,
    pub h_scrollbar_thumb: Option<char>,
    #[cfg(feature = "diff-view")]
    pub pin_scrollbar_focus: bool,
    pub h_scroll_offset: usize,
    pub h_scroll_override: Option<usize>,
    pub scroll_wheel: bool,
    pub scroll_wheel_multiplier: Option<u16>,

    // ── Interaction ──────────────────────────────────────────────────────
    pub focusable: bool,
    pub tab_stop: bool,
    pub on_focus: Option<Callback<()>>,
    pub on_blur: Option<Callback<()>>,
    pub on_scroll: Option<Callback<ScrollEvent>>,
    pub on_click: Option<Callback<DocumentClickEvent>>,
    pub on_select: Option<Callback<DocumentSelectEvent>>,
    pub on_key: Option<KeyHandler>,
    pub shared_selection_id: Option<Arc<str>>,

    // ── Code block highlighting ──────────────────────────────────────────
    #[cfg(feature = "syntax-syntect")]
    pub code_syntax_strategy: Option<Rc<dyn crate::widgets::TextAreaColorStrategy>>,

    // ── Custom gutter ────────────────────────────────────────────────────
    /// Per-logical-line custom gutter spans (0-based). When set, replaces the
    /// built-in `line_numbers` gutter.
    pub gutter_lines: Option<Arc<Vec<Vec<crate::style::Span>>>>,
    /// Fixed column width for the custom gutter. Overrides `line_numbers`
    /// gutter width when > 0.
    pub gutter_col_width: u16,
    /// Fixed empty cells between gutter and text content.
    pub gutter_gap: u16,
    /// Logical source-line indices (0-based) to exclude from clipboard copy.
    /// Byte ranges are derived at copy time from the visual cache.
    pub copy_excluded_source_lines: Option<Arc<Vec<usize>>>,
    #[cfg(feature = "diff-view")]
    /// Optional peer source lines used for split-view wrap synchronization.
    pub peer_source_lines: Option<Arc<Vec<Arc<str>>>>,
    #[cfg(feature = "diff-view")]
    pub split_wrap_sync: Option<crate::widgets::diff_view::SharedSplitWrapSync>,
    #[cfg(feature = "diff-view")]
    pub split_wrap_side: Option<crate::widgets::diff_view::SplitPaneSide>,
    #[cfg(feature = "diff-view")]
    pub diff_split_pane: Option<crate::widgets::DiffPane>,
    #[cfg(feature = "diff-view")]
    pub diff_context_separator_click:
        Option<crate::widgets::diff_view::DiffContextSeparatorClickConfig>,
    /// Style used for synthetic wrap-padding gutter rows inserted for peer sync.
    pub split_wrap_padding_gutter_style: Option<Style>,
    /// Style used for synthetic wrap-padding content rows inserted for peer sync.
    pub split_wrap_padding_style: Option<Style>,
    /// Enable word/line selection on double/triple click.
    pub multi_click_select: bool,
    /// Triple-click selection behavior.
    pub triple_click_mode: crate::widgets::TripleClickSelectionMode,
    /// Forward clicks to a wrapping `MouseRegion` (see [`DocumentView::passthrough_clicks`]).
    pub passthrough_clicks: bool,

    // ── Computed during reconciliation ────────────────────────────────────
    pub total_visual_lines: usize,
    pub max_line_width: u16,
    pub content_hash: u64,
    /// Selection cursor byte offset in `visual_cache.flat_text`.
    pub selection_cursor: usize,
    /// Selection anchor byte offset in `visual_cache.flat_text`.
    pub selection_anchor: Option<usize>,
    /// Optional rectangular table-cell selection.
    pub table_rect_selection: Option<DocumentTableRectSelection>,

    // ── Caches ───────────────────────────────────────────────────────────
    pub format_cache: FormatCache,
    pub visual_cache: VisualCache,
}

impl WidgetNode for DocumentViewNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }

    fn is_tab_stop(&self) -> bool {
        self.focusable && self.tab_stop
    }

    fn on_focus_callback(&self) -> Option<&Callback<()>> {
        self.on_focus.as_ref()
    }

    fn on_blur_callback(&self) -> Option<&Callback<()>> {
        self.on_blur.as_ref()
    }

    fn has_on_click(&self) -> bool {
        self.on_click.is_some()
            || self.has_diff_context_separator_click()
            || self.on_scroll.is_some()
            || self.scrollbar
            || self.h_scrollbar
            || self.scroll_wheel
    }

    fn hit_test_refinement(&self, _x: i16, _y: i16, _rect: Rect) -> Option<bool> {
        // Selection is mouse-driven even when the document is removed from focus
        // traversal and delegates wheel scrolling to an ancestor ScrollView.
        Some(true)
    }

    fn is_hoverable(&self) -> bool {
        self.on_click.is_some()
            || self.has_diff_context_separator_hover()
            || self.hover_style.has_explicit_style()
            || self.hover_border_style.is_some()
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        self.on_click.is_some()
            || self.has_diff_context_separator_hover()
            || self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
            || self.hover_border_style.is_some()
    }

    fn scrollbar_zones(
        &self,
        id: crate::core::node::NodeId,
        rect: Rect,
        parent_border_x: Option<i16>,
        parent_border_y: Option<i16>,
    ) -> Vec<ScrollbarZone> {
        let inner = rect.inner(self.border, self.padding);
        if inner.w == 0 || inner.h == 0 {
            return Vec::new();
        }

        // scrollbar_zones accounts for parent borders via the
        // `parent_border_x/y` parameters, so it uses a slightly broader
        // "over_border" condition than `content_layout` (which only knows
        // about the widget's own border).  We still delegate the gutter and
        // base content-width arithmetic to `content_layout`.
        let cl = self.content_layout(inner);

        compute_scrollbar_zones(ScrollbarZonesParams {
            id,
            rect,
            inner,
            border: self.border,
            scrollbar: self.scrollbar,
            scrollbar_variant: self.scrollbar_variant,
            scrollbar_gap: self.scrollbar_gap,
            h_scrollbar: self.h_scrollbar,
            h_scrollbar_variant: self.h_scrollbar_variant,
            content_x: cl.content_x,
            content_width: cl.content_width,
            max_content_width: self.max_line_width as usize,
            wrap: self.wrap,
            parent_border_x,
            parent_border_y,
        })
    }
}

impl DocumentViewNode {
    fn has_diff_context_separator_click(&self) -> bool {
        #[cfg(feature = "diff-view")]
        {
            self.diff_context_separator_click
                .as_ref()
                .is_some_and(|config| config.on_click.is_some())
        }
        #[cfg(not(feature = "diff-view"))]
        {
            false
        }
    }

    fn has_diff_context_separator_hover(&self) -> bool {
        #[cfg(feature = "diff-view")]
        {
            self.diff_context_separator_click
                .as_ref()
                .and_then(|config| config.hover_style)
                .is_some_and(|style| !style.is_empty())
        }
        #[cfg(not(feature = "diff-view"))]
        {
            false
        }
    }

    /// Number of lines used to size/render the line-number gutter.
    pub(crate) fn line_number_count(&self) -> usize {
        match self.line_number_mode {
            DocumentLineNumberMode::Visual => self.total_visual_lines.max(1),
            DocumentLineNumberMode::Source => self.value.split('\n').count().max(1),
        }
    }

    /// Effective gutter content width (excluding any configured gap).
    pub(crate) fn gutter_base_width(&self) -> u16 {
        if self.gutter_col_width > 0 {
            self.gutter_col_width
        } else if self.line_numbers {
            super::layout::gutter_width(
                self.line_number_count(),
                self.min_line_number_width,
                self.line_number_separator,
                self.line_number_content_gap,
            )
        } else {
            0
        }
    }

    /// Effective gutter width including the fixed gap before text content.
    pub(crate) fn gutter_width(&self) -> u16 {
        super::layout::gutter_total_width(self.gutter_base_width(), self.gutter_gap)
    }

    /// Compute the content-area layout from the padded/bordered `inner` rect.
    ///
    /// This centralises the gutter-width, scrollbar-column, and content-rect
    /// arithmetic that was previously duplicated across input, scrollbar, and
    /// event modules.
    pub(crate) fn content_layout(&self, inner: Rect) -> ContentLayout {
        let gutter_width = self.gutter_width();

        let v_scrollbar_over_border = self.scrollbar
            && matches!(self.scrollbar_variant, ScrollbarVariant::Integrated)
            && self.border;
        let scrollbar_cols: u16 = if self.scrollbar && !v_scrollbar_over_border {
            1u16.saturating_add(self.scrollbar_gap)
        } else {
            0
        };

        let content_width = inner
            .w
            .saturating_sub(gutter_width)
            .saturating_sub(scrollbar_cols);

        let h_scrollbar_over_border = self.h_scrollbar
            && matches!(self.h_scrollbar_variant, ScrollbarVariant::Integrated)
            && self.border;
        let h_scrollbar_visible = self.h_scrollbar
            && !self.wrap
            && (self.max_line_width as usize) > content_width as usize;

        let mut content_height = inner.h;
        if h_scrollbar_visible && !h_scrollbar_over_border {
            content_height = content_height.saturating_sub(1);
        }

        let content_x = inner.x.saturating_add(gutter_width as i16);
        let content_y = inner.y;

        ContentLayout {
            h_scrollbar_over_border,
            content_x,
            content_y,
            content_width,
            content_height,
        }
    }
}

impl From<DocumentView> for DocumentViewNode {
    fn from(dv: DocumentView) -> Self {
        Self {
            value: dv.value,
            content_type: dv.content_type,
            formatter: dv.formatter,
            wrap: dv.wrap,
            line_numbers: dv.line_numbers,
            min_line_number_width: dv.min_line_number_width,
            line_number_separator: dv.line_number_separator,
            line_number_content_gap: dv.line_number_content_gap,
            line_number_mode: dv.line_number_mode,
            highlight_full_width: dv.highlight_full_width,
            border: dv.border,
            border_style: dv.border_style,
            padding: dv.padding,
            table_wrap: dv.table_wrap,
            table_width_mode: dv.table_width_mode,
            table_outer_frame: dv.table_outer_frame,
            table_column_separators: dv.table_column_separators,
            table_row_separators: dv.table_row_separators,
            table_cell_padding: dv.table_cell_padding,
            table_border_variant: dv.table_border_variant,
            style: dv.style,
            line_number_style: dv.line_number_style,
            hover_style: dv.hover_style,
            focus_style: dv.focus_style,
            focus_content_style: dv.focus_content_style,
            selection_style: dv.selection_style,
            doc_styles: dv.doc_styles,
            hover_border_style: dv.hover_border_style,
            scroll_offset: dv.scroll_offset.unwrap_or(0),
            scroll_override: None,
            scroll_to_source_line: dv.scroll_to_source_line,
            cancelled_scroll_to_source_line: None,
            scroll_behavior: dv.scroll_behavior,
            smooth_scroll: SmoothScrollState::default(),
            scrollbar: dv.scrollbar,
            scrollbar_variant: dv.scrollbar_config.variant,
            scrollbar_gap: dv.scrollbar_config.gap,
            scrollbar_thumb_style: dv.scrollbar_config.thumb_style,
            scrollbar_thumb_focus_style: dv.scrollbar_config.thumb_focus_style,
            scrollbar_track_style: dv.scrollbar_config.track_style,
            scrollbar_thumb: dv.scrollbar_config.thumb,
            h_scrollbar: dv.h_scrollbar,
            h_scrollbar_variant: dv.h_scrollbar_variant,
            h_scrollbar_thumb: dv.h_scrollbar_thumb,
            #[cfg(feature = "diff-view")]
            pin_scrollbar_focus: dv.pin_scrollbar_focus_style,
            h_scroll_offset: 0,
            h_scroll_override: None,
            scroll_wheel: dv.scroll_wheel,
            scroll_wheel_multiplier: dv.scroll_wheel_multiplier,
            focusable: dv.focusable,
            tab_stop: dv.tab_stop,
            on_focus: dv.on_focus,
            on_blur: dv.on_blur,
            on_scroll: dv.on_scroll,
            on_click: dv.on_click,
            on_select: dv.on_select,
            on_key: dv.on_key,
            shared_selection_id: dv.shared_selection_id,
            #[cfg(feature = "syntax-syntect")]
            code_syntax_strategy: dv.code_syntax_strategy,
            gutter_lines: dv.gutter_lines,
            gutter_col_width: dv.gutter_col_width,
            gutter_gap: dv.gutter_gap,
            copy_excluded_source_lines: dv.copy_excluded_source_lines,
            #[cfg(feature = "diff-view")]
            peer_source_lines: dv.peer_source_lines,
            #[cfg(feature = "diff-view")]
            split_wrap_sync: dv.split_wrap_sync,
            #[cfg(feature = "diff-view")]
            split_wrap_side: dv.split_wrap_side,
            #[cfg(feature = "diff-view")]
            diff_split_pane: dv.diff_split_pane,
            #[cfg(feature = "diff-view")]
            diff_context_separator_click: dv.diff_context_separator_click,
            split_wrap_padding_gutter_style: dv.split_wrap_padding_gutter_style,
            split_wrap_padding_style: dv.split_wrap_padding_style,
            multi_click_select: dv.multi_click_select,
            triple_click_mode: dv.triple_click_mode,
            passthrough_clicks: dv.passthrough_clicks,
            total_visual_lines: 0,
            max_line_width: 0,
            content_hash: 0,
            selection_cursor: 0,
            selection_anchor: None,
            table_rect_selection: None,
            format_cache: FormatCache::default(),
            visual_cache: VisualCache::default(),
        }
    }
}

impl From<DocumentViewNode> for NodeKind {
    fn from(node: DocumentViewNode) -> Self {
        NodeKind::DocumentView(Box::new(node))
    }
}

/// Flatten the structured document into a flat list of visual lines.
///
/// This is width-aware: tables get column widths computed against `inner_w`,
/// and text lines are word-wrapped when `wrap` is enabled.
pub(crate) struct DocumentFlattenCtx<'a> {
    pub wrap: bool,
    pub table_wrap: bool,
    pub table_width_mode: DocumentTableWidthMode,
    pub table_outer_frame: bool,
    pub table_column_separators: bool,
    pub table_row_separators: super::TableRowSeparators,
    pub table_cell_padding: u16,
    pub table_border_variant: BorderStyle,
    pub doc_styles: &'a DocumentStyles,
    #[cfg(feature = "syntax-syntect")]
    pub code_strategy: Option<&'a dyn crate::widgets::TextAreaColorStrategy>,
}

struct DocumentFlattenAccum<'a> {
    out: &'a mut Vec<DocumentVisualLine>,
    max_w: &'a mut u16,
    next_table_id: &'a mut usize,
    bq_depth: u16,
    base_indent_cols: u16,
}

pub(crate) fn flatten_blocks(
    doc: &FormattedDocument,
    inner_w: u16,
    ctx: DocumentFlattenCtx<'_>,
) -> (Vec<DocumentVisualLine>, u16) {
    let mut lines = Vec::new();
    let mut max_w: u16 = 0;
    let mut next_table_id = 0usize;

    for block in &doc.blocks {
        flatten_block(
            block,
            inner_w,
            &ctx,
            &mut DocumentFlattenAccum {
                out: &mut lines,
                max_w: &mut max_w,
                next_table_id: &mut next_table_id,
                bq_depth: 0,
                base_indent_cols: 0,
            },
        );
    }

    // Merge adjacent spans with identical styles to reduce per-span overhead
    // during rendering (fewer Style::patch calls, fewer Arc allocs, fewer
    // buffer cell writes).
    for line in &mut lines {
        merge_visual_line_spans(line);
    }

    (lines, max_w)
}

/// Measure-only fast path: visual-line *counts* per source line plus the max
/// rendered width, for documents whose blocks are all [`FormattedBlock::Lines`]
/// (diffs and plain text). Returns `None` for documents containing tables, code
/// blocks, blockquotes, etc. so the caller falls back to the full
/// [`flatten_blocks`] path that those block kinds require.
///
/// The per-source counts mirror [`flatten_block`]'s `Lines` arm exactly (same
/// wrap predicate, same available-width math, same `count_wrapped_lines_for_budgets`
/// which tracks `wrap_spans_for_budgets`), so the returned heights equal
/// `source_visual_heights(&flatten_blocks(...).0)` and the summed count equals
/// `flatten_blocks(...).0.len()`. The result carries no spans, avoiding the
/// per-line clone/alloc that dominates the measure path during resize.
pub(crate) fn lines_only_source_heights(
    doc: &FormattedDocument,
    inner_w: u16,
    wrap: bool,
) -> Option<(Vec<u16>, u16)> {
    let mut max_source = 0usize;
    for block in &doc.blocks {
        let super::FormattedBlock::Lines(lines) = block else {
            return None;
        };
        for fl in lines {
            max_source = max_source.max(fl.source_line);
        }
    }

    let mut heights = vec![0u16; max_source.saturating_add(1)];
    let mut max_w: u16 = 0;
    for block in &doc.blocks {
        let super::FormattedBlock::Lines(lines) = block else {
            unreachable!("validated above");
        };
        for fl in lines {
            // base_indent_cols is 0 here: a Lines-only document is never nested
            // inside a blockquote (which would yield non-Lines blocks).
            let indent_cols = fl.indent.saturating_mul(2);
            let line_w = span_width(&fl.spans);
            max_w = max_w.max(line_w.saturating_add(indent_cols));

            let count = if wrap && line_w + indent_cols > inner_w {
                let avail = inner_w.saturating_sub(indent_cols);
                crate::utils::text::count_wrapped_lines_for_budgets(&fl.spans, avail, avail) as u16
            } else {
                1
            };
            heights[fl.source_line] = heights[fl.source_line].saturating_add(count);
        }
    }

    Some((heights, max_w))
}

pub(crate) const CODE_BLOCK_LEFT_INSET_COLS: u16 = 1;

/// Display columns rendered before selectable content (code inset, blockquote bars).
pub(crate) fn visual_line_render_prefix_cols(vline: &DocumentVisualLine) -> usize {
    match &vline.kind {
        VisualLineKind::CodeLine { .. } => CODE_BLOCK_LEFT_INSET_COLS as usize,
        VisualLineKind::BlockQuoteLine { depth, .. } => (*depth as usize).saturating_mul(2),
        _ => 0,
    }
}

/// UTF-8 bytes in the render-only prefix for a visual line.
pub(crate) fn visual_line_render_prefix_bytes(vline: &DocumentVisualLine) -> usize {
    match &vline.kind {
        VisualLineKind::CodeLine { .. } => CODE_BLOCK_LEFT_INSET_COLS as usize,
        VisualLineKind::BlockQuoteLine { depth, .. } => "│ ".len().saturating_mul(*depth as usize),
        _ => 0,
    }
}

/// Map a rendered content-area column to a byte offset within `line_text`.
pub(crate) fn document_view_byte_in_line_from_visual_col(
    line_text: &str,
    vline: &DocumentVisualLine,
    visual_col: usize,
) -> usize {
    let content_col = visual_col.saturating_sub(visual_line_render_prefix_cols(vline));
    crate::utils::text::byte_at_col(line_text, content_col)
}

/// Merge consecutive spans that share the same `Style` into a single span.
/// This significantly reduces span count for syntax-highlighted code where
/// adjacent tokens often resolve to the same colour.
fn merge_spans(spans: &mut Vec<Span>) {
    if spans.len() <= 1 {
        return;
    }
    let mut merged = Vec::with_capacity(spans.len());
    let mut drain = spans.drain(..);
    let mut current = drain.next().unwrap();
    for next in drain {
        if current.style == next.style && current.row_style_policy == next.row_style_policy {
            // Coalesce: concatenate content.
            let mut combined = String::with_capacity(current.content.len() + next.content.len());
            combined.push_str(&current.content);
            combined.push_str(&next.content);
            current = Span {
                content: Arc::from(combined),
                style: current.style,
                row_style_policy: current.row_style_policy,
            };
        } else {
            merged.push(current);
            current = next;
        }
    }
    merged.push(current);
    *spans = merged;
}

/// Apply span merging to all span vectors inside a visual line.
fn merge_visual_line_spans(line: &mut DocumentVisualLine) {
    match &mut line.kind {
        VisualLineKind::Text { spans, .. }
        | VisualLineKind::BlockQuoteLine { spans, .. }
        | VisualLineKind::CodeLine { spans, .. }
        | VisualLineKind::DiagramRow { spans, .. } => {
            merge_spans(spans);
        }
        VisualLineKind::TableRow { cells, .. } => {
            for cell in cells.iter_mut() {
                merge_spans(cell);
            }
        }
        VisualLineKind::TableBorder { .. } | VisualLineKind::HorizontalRule => {}
    }
}

/// Extract the anchor span (empty span with bg at index 0) that
/// `full_row_bg_style` uses for the row-wide background.
/// `wrap_spans_for_budgets` drops empty spans, so callers re-inject
/// this anchor into each wrapped visual line.
fn extract_bg_anchor(spans: &[Span]) -> Option<Span> {
    spans
        .first()
        .filter(|s| s.content.is_empty() && s.style.bg.is_some())
        .cloned()
}

fn flatten_block(
    block: &super::FormattedBlock,
    inner_w: u16,
    ctx: &DocumentFlattenCtx<'_>,
    accum: &mut DocumentFlattenAccum<'_>,
) {
    let wrap = ctx.wrap;
    let doc_styles = ctx.doc_styles;
    #[cfg(feature = "syntax-syntect")]
    let code_strategy = ctx.code_strategy;
    let bq_depth = accum.bq_depth;
    let base_indent_cols = accum.base_indent_cols;
    match block {
        super::FormattedBlock::Lines(formatted_lines) => {
            for fl in formatted_lines {
                let indent_cols = base_indent_cols.saturating_add(fl.indent * 2);
                let line_w = span_width(&fl.spans);
                *accum.max_w = (*accum.max_w).max(line_w + indent_cols);

                if wrap && line_w + indent_cols > inner_w {
                    // Word-wrap this line
                    let avail = inner_w.saturating_sub(indent_cols);
                    let wrapped =
                        crate::utils::text::wrap_spans_for_budgets(&fl.spans, avail, avail);
                    let anchor = extract_bg_anchor(&fl.spans);
                    for (i, mut wline) in wrapped.into_iter().enumerate() {
                        if let Some(ref anchor) = anchor {
                            wline.insert(0, anchor.clone());
                        }
                        accum.out.push(DocumentVisualLine {
                            kind: if bq_depth > 0 {
                                VisualLineKind::BlockQuoteLine {
                                    spans: wline,
                                    depth: bq_depth,
                                    links: Vec::new(),
                                }
                            } else {
                                VisualLineKind::Text {
                                    spans: wline,
                                    indent_cols,
                                    continuation: i > 0,
                                    links: Vec::new(),
                                }
                            },
                            source_line: fl.source_line,
                        });
                    }
                } else {
                    accum.out.push(DocumentVisualLine {
                        kind: if bq_depth > 0 {
                            VisualLineKind::BlockQuoteLine {
                                spans: fl.spans.clone(),
                                depth: bq_depth,
                                links: fl.links.clone(),
                            }
                        } else {
                            let indent_bytes = indent_cols as usize;
                            VisualLineKind::Text {
                                spans: fl.spans.clone(),
                                indent_cols,
                                continuation: false,
                                links: shift_links(&fl.links, indent_bytes),
                            }
                        },
                        source_line: fl.source_line,
                    });
                }
            }
        }

        super::FormattedBlock::Table(table) => {
            flatten_table(table, inner_w, ctx, accum);
        }

        super::FormattedBlock::CodeBlock(cb) => {
            flatten_code_block(
                cb,
                CodeFlattenCtx {
                    inner_w,
                    wrap,
                    doc_styles,
                    base_indent_cols: accum.base_indent_cols,
                    #[cfg(feature = "syntax-syntect")]
                    code_strategy,
                },
                accum.out,
                accum.max_w,
            );
        }

        super::FormattedBlock::Diagram(diagram) => {
            flatten_diagram(
                diagram,
                doc_styles,
                accum.out,
                accum.max_w,
                accum.base_indent_cols,
            );
        }

        super::FormattedBlock::HorizontalRule { source_line } => {
            *accum.max_w = (*accum.max_w).max(3);
            accum.out.push(DocumentVisualLine {
                kind: VisualLineKind::HorizontalRule,
                source_line: *source_line,
            });
        }

        super::FormattedBlock::BlockQuote(bq) => {
            for nested in &bq.blocks {
                let prev_bq_depth = accum.bq_depth;
                accum.bq_depth = bq_depth.saturating_add(bq.depth);
                flatten_block(nested, inner_w.saturating_sub(bq.depth * 2 + 1), ctx, accum);
                accum.bq_depth = prev_bq_depth;
            }
        }

        super::FormattedBlock::List(list) => {
            for (idx, item) in list.items.iter().enumerate() {
                // Build bullet/number prefix
                let prefix = if list.ordered {
                    format!("{}. ", list.start + idx)
                } else {
                    "\u{2022} ".to_string() // bullet
                };

                // First block of item gets the prefix, rest get indent
                let prefix_len = unicode_width::UnicodeWidthStr::width(prefix.as_str()) as u16;
                for (bi, sub_block) in item.content.iter().enumerate() {
                    if bi == 0 {
                        // Prepend prefix to the first line
                        if let super::FormattedBlock::Lines(flines) = sub_block {
                            for (li, fl) in flines.iter().enumerate() {
                                let spans = if li == 0 {
                                    let marker_style = if list.ordered {
                                        doc_styles.list_enumeration_style
                                    } else {
                                        doc_styles.list_item_style
                                    };
                                    let mut s = vec![Span::new(prefix.clone()).style(marker_style)];
                                    s.extend(fl.spans.iter().cloned());
                                    s
                                } else {
                                    fl.spans.clone()
                                };
                                let inner_first = fl.indent * 2;
                                let inner_cont = fl.indent * 2 + prefix_len;

                                let indent_cols = base_indent_cols.saturating_add(if li == 0 {
                                    inner_first
                                } else {
                                    inner_cont
                                });

                                let cont_indent_cols = base_indent_cols.saturating_add(inner_cont);

                                let line_w = span_width(&spans);
                                *accum.max_w = (*accum.max_w).max(line_w + indent_cols);

                                // Use continuation indent for wrap budget so text
                                // fits on continuation lines which are indented more.
                                let first_budget = inner_w.saturating_sub(indent_cols);
                                let cont_budget = inner_w.saturating_sub(cont_indent_cols);
                                let should_wrap = wrap && line_w + indent_cols > inner_w;

                                if bq_depth > 0 {
                                    let line_prefix_bytes =
                                        if li == 0 { prefix.len() } else { 0usize };
                                    if should_wrap {
                                        let anchor = extract_bg_anchor(&fl.spans);
                                        for mut wline in crate::utils::text::wrap_spans_for_budgets(
                                            &spans,
                                            first_budget,
                                            cont_budget,
                                        ) {
                                            if let Some(ref anchor) = anchor {
                                                wline.insert(0, anchor.clone());
                                            }
                                            accum.out.push(DocumentVisualLine {
                                                kind: VisualLineKind::BlockQuoteLine {
                                                    spans: wline,
                                                    depth: bq_depth,
                                                    links: Vec::new(),
                                                },
                                                source_line: fl.source_line,
                                            });
                                        }
                                    } else {
                                        accum.out.push(DocumentVisualLine {
                                            kind: VisualLineKind::BlockQuoteLine {
                                                spans,
                                                depth: bq_depth,
                                                links: shift_links(&fl.links, line_prefix_bytes),
                                            },
                                            source_line: fl.source_line,
                                        });
                                    }
                                } else if should_wrap {
                                    let wrapped = crate::utils::text::wrap_spans_for_budgets(
                                        &spans,
                                        first_budget,
                                        cont_budget,
                                    );
                                    let anchor = extract_bg_anchor(&fl.spans);
                                    for (wi, mut wline) in wrapped.into_iter().enumerate() {
                                        if let Some(ref anchor) = anchor {
                                            wline.insert(0, anchor.clone());
                                        }
                                        let w_indent_cols = if wi == 0 {
                                            indent_cols
                                        } else {
                                            cont_indent_cols
                                        };
                                        accum.out.push(DocumentVisualLine {
                                            kind: VisualLineKind::Text {
                                                spans: wline,
                                                indent_cols: w_indent_cols,
                                                continuation: li > 0 || wi > 0,
                                                links: Vec::new(),
                                            },
                                            source_line: fl.source_line,
                                        });
                                    }
                                } else {
                                    let links = if li == 0 {
                                        shift_links(&fl.links, prefix.len())
                                    } else {
                                        fl.links.clone()
                                    };
                                    accum.out.push(DocumentVisualLine {
                                        kind: VisualLineKind::Text {
                                            spans,
                                            indent_cols,
                                            continuation: li > 0,
                                            links,
                                        },
                                        source_line: fl.source_line,
                                    });
                                }
                            }
                        } else {
                            let prev_base_indent = accum.base_indent_cols;
                            accum.base_indent_cols = base_indent_cols.saturating_add(prefix_len);
                            flatten_block(sub_block, inner_w, ctx, accum);
                            accum.base_indent_cols = prev_base_indent;
                        }
                    } else {
                        let prev_base_indent = accum.base_indent_cols;
                        accum.base_indent_cols = base_indent_cols.saturating_add(prefix_len);
                        flatten_block(sub_block, inner_w, ctx, accum);
                        accum.base_indent_cols = prev_base_indent;
                    }
                }
            }
        }
    }
}

fn flatten_table(
    table: &super::FormattedTable,
    inner_w: u16,
    ctx: &DocumentFlattenCtx<'_>,
    accum: &mut DocumentFlattenAccum<'_>,
) {
    let table_wrap = ctx.table_wrap;
    let table_width_mode = ctx.table_width_mode;
    let table_outer_frame = ctx.table_outer_frame;
    let table_column_separators = ctx.table_column_separators;
    let table_row_separators = ctx.table_row_separators;
    let table_cell_padding = ctx.table_cell_padding;
    let table_border_variant = ctx.table_border_variant;
    let doc_styles = ctx.doc_styles;
    let table_id = *accum.next_table_id;
    *accum.next_table_id = accum.next_table_id.saturating_add(1);

    let ncols = table.alignments.len().max(
        table
            .headers
            .len()
            .max(table.rows.first().map_or(0, |r| r.len())),
    );
    if ncols == 0 {
        return;
    }

    // Compute natural content widths.
    let mut content_widths = vec![1u16; ncols];
    for (i, hdr) in table.headers.iter().enumerate() {
        if i < ncols {
            content_widths[i] = content_widths[i].max(span_width(hdr));
        }
    }
    for row in &table.rows {
        for (i, cell) in row.iter().enumerate() {
            if i < ncols {
                content_widths[i] = content_widths[i].max(span_width(cell));
            }
        }
    }

    let pad2 = table_cell_padding.saturating_mul(2);
    let mut widths: Vec<u16> = content_widths
        .iter()
        .map(|w| w.saturating_add(pad2))
        .collect();

    let fixed_chars = table_fixed_chars(ncols, table_outer_frame, table_column_separators);
    let mut total_w = table_render_width(&widths, table_outer_frame, table_column_separators);
    if matches!(table_width_mode, DocumentTableWidthMode::Fill) && inner_w > total_w {
        distribute_extra_width(&mut widths, inner_w.saturating_sub(total_w));
        total_w = table_render_width(&widths, table_outer_frame, table_column_separators);
    }
    if table_wrap && total_w > inner_w && inner_w > fixed_chars {
        let target_cols_total = inner_w.saturating_sub(fixed_chars);
        let min_col_total = pad2.saturating_add(1);
        shrink_widths_to_fit(&mut widths, target_cols_total, min_col_total);
        total_w = table_render_width(&widths, table_outer_frame, table_column_separators);
    }

    *accum.max_w = (*accum.max_w).max(total_w);

    let aligns: Vec<super::ColumnAlign> = (0..ncols)
        .map(|i| {
            table
                .alignments
                .get(i)
                .copied()
                .unwrap_or(super::ColumnAlign::Left)
        })
        .collect();

    let source = table.source_line_start;
    let mut logical_row_index = 0usize;

    // Top border
    if table_outer_frame {
        accum.out.push(DocumentVisualLine {
            kind: VisualLineKind::TableBorder {
                kind: TableBorderKind::Top,
                widths: widths.clone(),
                border_variant: table_border_variant,
                outer_frame: table_outer_frame,
                column_separators: table_column_separators,
            },
            source_line: source,
        });
    }

    // Header row
    if !table.headers.is_empty() {
        let cells: Vec<Vec<Span>> = (0..ncols)
            .map(|i| {
                let mut cell = table
                    .headers
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| vec![Span::new("")]);
                // Apply header bold style
                for span in &mut cell {
                    span.style = doc_styles.table_header_style.patch(span.style);
                }
                cell
            })
            .collect();

        let full_cell_texts: Vec<Arc<str>> = cells
            .iter()
            .map(|cell| Arc::from(spans_plain_text(cell)))
            .collect();
        let wrapped_cells: Vec<Vec<Vec<Span>>> = cells
            .iter()
            .zip(widths.iter())
            .map(|(cell, &col_w)| {
                if table_wrap {
                    let content_w = col_w
                        .saturating_sub(table_cell_padding.saturating_mul(2))
                        .max(1);
                    crate::utils::text::wrap_spans_for_budgets(cell, content_w, content_w)
                } else {
                    vec![cell.clone()]
                }
            })
            .collect();
        let row_line_count = wrapped_cells.iter().map(Vec::len).max().unwrap_or(1);

        for row_line_index in 0..row_line_count {
            let row_cells: Vec<Vec<Span>> = wrapped_cells
                .iter()
                .map(|wrapped| {
                    wrapped
                        .get(row_line_index)
                        .cloned()
                        .unwrap_or_else(|| vec![Span::new("")])
                })
                .collect();
            let cell_line_texts: Vec<Arc<str>> = row_cells
                .iter()
                .map(|cell| Arc::from(spans_plain_text(cell)))
                .collect();

            accum.out.push(DocumentVisualLine {
                kind: VisualLineKind::TableRow {
                    cell_line_texts,
                    full_cell_texts: full_cell_texts.clone(),
                    cells: row_cells,
                    alignments: aligns.clone(),
                    widths: widths.clone(),
                    table_id,
                    row_index: logical_row_index,
                    row_line_index,
                    border_variant: table_border_variant,
                    outer_frame: table_outer_frame,
                    column_separators: table_column_separators,
                    cell_padding: table_cell_padding,
                },
                source_line: source,
            });
        }
        logical_row_index = logical_row_index.saturating_add(1);

        // Mid border (between header and data)
        if matches!(
            table_row_separators,
            super::TableRowSeparators::Header | super::TableRowSeparators::All
        ) {
            accum.out.push(DocumentVisualLine {
                kind: VisualLineKind::TableBorder {
                    kind: TableBorderKind::Mid,
                    widths: widths.clone(),
                    border_variant: table_border_variant,
                    outer_frame: table_outer_frame,
                    column_separators: table_column_separators,
                },
                source_line: source,
            });
        }
    }

    // Data rows
    for (ri, row) in table.rows.iter().enumerate() {
        let cells: Vec<Vec<Span>> = (0..ncols)
            .map(|i| row.get(i).cloned().unwrap_or_else(|| vec![Span::new("")]))
            .collect();
        let full_cell_texts: Vec<Arc<str>> = cells
            .iter()
            .map(|cell| Arc::from(spans_plain_text(cell)))
            .collect();
        let wrapped_cells: Vec<Vec<Vec<Span>>> = cells
            .iter()
            .zip(widths.iter())
            .map(|(cell, &col_w)| {
                if table_wrap {
                    let content_w = col_w
                        .saturating_sub(table_cell_padding.saturating_mul(2))
                        .max(1);
                    crate::utils::text::wrap_spans_for_budgets(cell, content_w, content_w)
                } else {
                    vec![cell.clone()]
                }
            })
            .collect();
        let row_line_count = wrapped_cells.iter().map(Vec::len).max().unwrap_or(1);

        let row_source = source
            .saturating_add(ri)
            .saturating_add(if table.headers.is_empty() { 0 } else { 2 });

        for row_line_index in 0..row_line_count {
            let row_cells: Vec<Vec<Span>> = wrapped_cells
                .iter()
                .map(|wrapped| {
                    wrapped
                        .get(row_line_index)
                        .cloned()
                        .unwrap_or_else(|| vec![Span::new("")])
                })
                .collect();
            let cell_line_texts: Vec<Arc<str>> = row_cells
                .iter()
                .map(|cell| Arc::from(spans_plain_text(cell)))
                .collect();

            accum.out.push(DocumentVisualLine {
                kind: VisualLineKind::TableRow {
                    cell_line_texts,
                    full_cell_texts: full_cell_texts.clone(),
                    cells: row_cells,
                    alignments: aligns.clone(),
                    widths: widths.clone(),
                    table_id,
                    row_index: logical_row_index,
                    row_line_index,
                    border_variant: table_border_variant,
                    outer_frame: table_outer_frame,
                    column_separators: table_column_separators,
                    cell_padding: table_cell_padding,
                },
                source_line: row_source,
            });
        }
        logical_row_index = logical_row_index.saturating_add(1);

        // Mid border between data rows
        if matches!(table_row_separators, super::TableRowSeparators::All)
            && ri + 1 < table.rows.len()
        {
            accum.out.push(DocumentVisualLine {
                kind: VisualLineKind::TableBorder {
                    kind: TableBorderKind::Mid,
                    widths: widths.clone(),
                    border_variant: table_border_variant,
                    outer_frame: table_outer_frame,
                    column_separators: table_column_separators,
                },
                source_line: row_source,
            });
        }
    }

    // Bottom border
    if table_outer_frame {
        accum.out.push(DocumentVisualLine {
            kind: VisualLineKind::TableBorder {
                kind: TableBorderKind::Bottom,
                widths,
                border_variant: table_border_variant,
                outer_frame: table_outer_frame,
                column_separators: table_column_separators,
            },
            source_line: source,
        });
    }
}

struct CodeFlattenCtx<'a> {
    inner_w: u16,
    wrap: bool,
    doc_styles: &'a DocumentStyles,
    base_indent_cols: u16,
    #[cfg(feature = "syntax-syntect")]
    code_strategy: Option<&'a dyn crate::widgets::TextAreaColorStrategy>,
}

#[derive(Clone, Copy)]
struct CodeVisualCtx {
    inner_w: u16,
    wrap: bool,
    base_indent_cols: u16,
}

fn flatten_code_block(
    cb: &super::FormattedCodeBlock,
    ctx: CodeFlattenCtx<'_>,
    out: &mut Vec<DocumentVisualLine>,
    max_w: &mut u16,
) {
    if let Some(lang) = cb.language.as_deref()
        && (lang.eq_ignore_ascii_case("diff") || lang.eq_ignore_ascii_case("patch"))
    {
        flatten_diff_code_block(cb, &ctx, out, max_w);
        return;
    }

    let block_style = ctx.doc_styles.code_block_style;

    // Try syntax highlighting via SyntectStrategy
    #[cfg(feature = "syntax-syntect")]
    let highlighted: Option<Vec<Vec<Span>>> = ctx.code_strategy.and_then(|strategy| {
        let input = crate::widgets::TextAreaColorInput {
            value: &cb.code,
            language: cb.language.as_deref(),
            theme: None,
        };
        let result = strategy.highlight(input);
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    });
    #[cfg(not(feature = "syntax-syntect"))]
    let highlighted: Option<Vec<Vec<Span>>> = None;

    let code_lines: Vec<&str> = cb.code.split('\n').collect();
    for (i, line_text) in code_lines.iter().enumerate() {
        let spans = if let Some(ref hl_lines) = highlighted {
            hl_lines
                .get(i)
                .cloned()
                .unwrap_or_else(|| vec![Span::new(*line_text)])
        } else {
            vec![Span::new(*line_text)]
        };

        push_code_visual_lines(
            spans,
            block_style,
            cb.source_line_start.saturating_add(i),
            CodeVisualCtx {
                inner_w: ctx.inner_w,
                wrap: ctx.wrap,
                base_indent_cols: ctx.base_indent_cols,
            },
            out,
            max_w,
        );
    }
}

fn flatten_diff_code_block(
    cb: &super::FormattedCodeBlock,
    ctx: &CodeFlattenCtx<'_>,
    out: &mut Vec<DocumentVisualLine>,
    max_w: &mut u16,
) {
    let base_block_style = ctx.doc_styles.code_block_style;
    let palette = &ctx.doc_styles.diff_palette;

    for (i, line_text) in cb.code.split('\n').enumerate() {
        let (line_bg_style, spans) = classify_diff_line(line_text, palette);
        let block_style = base_block_style.patch(line_bg_style);

        push_code_visual_lines(
            spans,
            block_style,
            cb.source_line_start.saturating_add(i),
            CodeVisualCtx {
                inner_w: ctx.inner_w,
                wrap: ctx.wrap,
                base_indent_cols: ctx.base_indent_cols,
            },
            out,
            max_w,
        );
    }
}

fn push_code_visual_lines(
    spans: Vec<Span>,
    block_style: Style,
    source_line: usize,
    ctx: CodeVisualCtx,
    out: &mut Vec<DocumentVisualLine>,
    max_w: &mut u16,
) {
    let content_line_w = span_width(&spans).saturating_add(CODE_BLOCK_LEFT_INSET_COLS);
    let line_w = content_line_w.saturating_add(ctx.base_indent_cols);
    let available_w = ctx.inner_w.saturating_sub(ctx.base_indent_cols);
    if ctx.wrap && line_w > ctx.inner_w {
        let content_w = available_w.saturating_sub(CODE_BLOCK_LEFT_INSET_COLS);
        for wrapped in crate::utils::text::wrap_spans_for_budgets(&spans, content_w, content_w) {
            let wrapped = with_visual_indent(wrapped, ctx.base_indent_cols);
            let wrapped_w = span_width(&wrapped).saturating_add(CODE_BLOCK_LEFT_INSET_COLS);
            *max_w = (*max_w).max(wrapped_w);
            out.push(DocumentVisualLine {
                kind: VisualLineKind::CodeLine {
                    spans: wrapped,
                    block_style,
                },
                source_line,
            });
        }
    } else {
        *max_w = (*max_w).max(line_w);
        let spans = with_visual_indent(spans, ctx.base_indent_cols);
        out.push(DocumentVisualLine {
            kind: VisualLineKind::CodeLine { spans, block_style },
            source_line,
        });
    }
}

fn classify_diff_line(line: &str, palette: &crate::style::DiffPalette) -> (Style, Vec<Span>) {
    if line.starts_with("@@") {
        return (
            Style::default(),
            vec![Span::new(line).style(palette.patch_header)],
        );
    }
    if line.starts_with("+++") || line.starts_with("---") {
        return (
            Style::default(),
            vec![Span::new(line).style(palette.patch_header)],
        );
    }
    if line.starts_with("diff ") || line.starts_with("index ") {
        return (
            Style::default(),
            vec![Span::new(line).style(palette.patch_header)],
        );
    }
    if let Some(rest) = line.strip_prefix('+') {
        let mut spans = Vec::with_capacity(2);
        spans.push(Span::new("+").style(palette.added_marker));
        if !rest.is_empty() {
            spans.push(Span::new(rest.to_string()));
        }
        return (palette.added, spans);
    }
    if let Some(rest) = line.strip_prefix('-') {
        let mut spans = Vec::with_capacity(2);
        spans.push(Span::new("-").style(palette.removed_marker));
        if !rest.is_empty() {
            spans.push(Span::new(rest.to_string()));
        }
        return (palette.removed, spans);
    }
    (palette.context, vec![Span::new(line)])
}

fn flatten_diagram(
    diagram: &super::FormattedDiagramBlock,
    doc_styles: &DocumentStyles,
    out: &mut Vec<DocumentVisualLine>,
    max_w: &mut u16,
    base_indent_cols: u16,
) {
    let rows = super::diagram::rasterize_diagram(&diagram.diagram, doc_styles);
    let rows = if rows.is_empty() {
        vec![vec![Span::new("")]]
    } else {
        rows
    };
    for mut spans in rows {
        for span in &mut spans {
            span.style = doc_styles.code_block_style.patch(span.style);
        }
        spans = with_visual_indent(spans, base_indent_cols);
        *max_w = (*max_w).max(span_width(&spans));
        out.push(DocumentVisualLine {
            kind: VisualLineKind::DiagramRow {
                spans,
                source_text: Some(diagram.source_code.clone()),
            },
            source_line: diagram.source_line_start,
        });
    }
}

fn with_visual_indent(mut spans: Vec<Span>, indent_cols: u16) -> Vec<Span> {
    if indent_cols > 0 {
        spans.insert(0, Span::new(" ".repeat(indent_cols as usize)));
    }
    spans
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Calculate the display width of a sequence of spans.
pub(crate) fn span_width(spans: &[Span]) -> u16 {
    spans
        .iter()
        .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()) as u16)
        .sum()
}
fn spans_plain_text(spans: &[Span]) -> String {
    spans.iter().map(|s| s.content.as_ref()).collect()
}

fn shift_links(links: &[FormattedLink], shift_bytes: usize) -> Vec<FormattedLink> {
    links
        .iter()
        .map(|link| FormattedLink {
            start: link.start.saturating_add(shift_bytes),
            end: link.end.saturating_add(shift_bytes),
            url: link.url.clone(),
        })
        .collect()
}

/// Build per-line/plain-text byte index data for selection and hit-testing.
pub(crate) fn build_visual_text_index(
    lines: &[DocumentVisualLine],
    hr_width: u16,
) -> (Vec<Arc<str>>, Vec<usize>, Vec<usize>, Arc<str>) {
    let mut line_texts = Vec::with_capacity(lines.len());
    let mut line_starts = Vec::with_capacity(lines.len());
    let mut line_lengths = Vec::with_capacity(lines.len());
    let mut flat = String::new();
    let mut active_diagram_start: Option<(usize, usize)> = None;

    for (i, line) in lines.iter().enumerate() {
        let text = visual_line_plain_text(line, hr_width);
        let is_diagram_row = matches!(line.kind, VisualLineKind::DiagramRow { .. });
        let is_diagram_continuation = is_diagram_row
            && i > 0
            && lines[i - 1].source_line == line.source_line
            && matches!(lines[i - 1].kind, VisualLineKind::DiagramRow { .. });
        let start = if is_diagram_continuation {
            active_diagram_start
                .filter(|(source_line, _)| *source_line == line.source_line)
                .map(|(_, start)| start)
                .unwrap_or_else(|| flat.len())
        } else {
            let start = flat.len();
            if is_diagram_row {
                active_diagram_start = Some((line.source_line, start));
            } else {
                active_diagram_start = None;
            }
            start
        };
        let len = text.len();
        if !is_diagram_continuation {
            flat.push_str(&text);
        }
        let next_is_diagram_continuation = lines.get(i + 1).is_some_and(|next| {
            is_diagram_row
                && next.source_line == line.source_line
                && matches!(next.kind, VisualLineKind::DiagramRow { .. })
        });
        if i + 1 < lines.len() && !next_is_diagram_continuation {
            flat.push('\n');
        }
        line_starts.push(start);
        line_lengths.push(len);
        line_texts.push(Arc::from(text));
    }

    (line_texts, line_starts, line_lengths, Arc::from(flat))
}

#[cfg(feature = "diff-view")]
pub(crate) fn source_visual_heights(lines: &[DocumentVisualLine]) -> Vec<u16> {
    let max_source = lines.iter().map(|line| line.source_line).max().unwrap_or(0);
    let mut heights = vec![0u16; max_source.saturating_add(1)];
    for line in lines {
        heights[line.source_line] = heights[line.source_line].saturating_add(1);
    }
    heights
}

#[cfg(feature = "diff-view")]
pub(crate) fn insert_source_padding_visual_lines(
    lines: &mut Vec<DocumentVisualLine>,
    padding: &[u16],
) {
    if lines.is_empty() || padding.is_empty() {
        return;
    }

    let mut out = Vec::with_capacity(
        lines.len().saturating_add(
            padding
                .iter()
                .fold(0usize, |acc, count| acc.saturating_add(*count as usize)),
        ),
    );

    let mut idx = 0usize;
    while idx < lines.len() {
        let source_line = lines[idx].source_line;
        while idx < lines.len() && lines[idx].source_line == source_line {
            out.push(lines[idx].clone());
            idx += 1;
        }

        let pad_count = padding.get(source_line).copied().unwrap_or(0);
        for _ in 0..pad_count {
            out.push(DocumentVisualLine {
                kind: VisualLineKind::Text {
                    spans: Vec::new(),
                    indent_cols: 0,
                    continuation: true,
                    links: Vec::new(),
                },
                source_line,
            });
        }
    }

    *lines = out;
}

/// Convert a visual line into its plain rendered text (content area only).
pub(crate) fn visual_line_plain_text(vline: &DocumentVisualLine, hr_width: u16) -> String {
    match &vline.kind {
        VisualLineKind::Text {
            spans, indent_cols, ..
        } => {
            let mut out = " ".repeat(*indent_cols as usize);
            for span in spans {
                out.push_str(span.content.as_ref());
            }
            out
        }
        VisualLineKind::TableRow {
            cells,
            alignments,
            widths,
            border_variant,
            outer_frame,
            column_separators,
            cell_padding,
            ..
        } => {
            let glyphs = table_border_glyphs(*border_variant);
            let mut out = String::new();
            if *outer_frame {
                out.push_str(glyphs.left);
            }
            for (i, (cell, &w)) in cells.iter().zip(widths.iter()).enumerate() {
                let align = alignments
                    .get(i)
                    .copied()
                    .unwrap_or(super::ColumnAlign::Left);
                let content: String = cell.iter().map(|s| s.content.as_ref()).collect();
                let content_w = unicode_width::UnicodeWidthStr::width(content.as_str());
                let content_col_w =
                    (w as usize).saturating_sub(cell_padding.saturating_mul(2) as usize);
                let pad = content_col_w.saturating_sub(content_w);
                let (lpad, rpad) = match align {
                    super::ColumnAlign::Left => (0, pad),
                    super::ColumnAlign::Right => (pad, 0),
                    super::ColumnAlign::Center => (pad / 2, pad - pad / 2),
                };
                if *cell_padding > 0 {
                    out.push_str(&" ".repeat(*cell_padding as usize));
                }
                out.push_str(&" ".repeat(lpad));
                out.push_str(&content);
                out.push_str(&" ".repeat(rpad));
                if *cell_padding > 0 {
                    out.push_str(&" ".repeat(*cell_padding as usize));
                }
                let has_next = i + 1 < widths.len();
                if has_next && *column_separators {
                    out.push_str(glyphs.center);
                }
            }
            if *outer_frame {
                out.push_str(glyphs.right);
            }
            out
        }
        VisualLineKind::TableBorder {
            kind,
            widths,
            border_variant,
            outer_frame,
            column_separators,
        } => {
            let glyphs = table_border_glyphs(*border_variant);
            let kind = match kind {
                TableBorderKind::Top => TableBorderLineKind::Top,
                TableBorderKind::Mid => TableBorderLineKind::Mid,
                TableBorderKind::Bottom => TableBorderLineKind::Bottom,
            };
            table_border_line(kind, widths, glyphs, *outer_frame, *column_separators)
        }
        VisualLineKind::HorizontalRule => "─".repeat(hr_width.max(1) as usize),
        VisualLineKind::DiagramRow {
            source_text: Some(source),
            ..
        } => source.to_string(),
        VisualLineKind::DiagramRow {
            source_text: None, ..
        } => String::new(),
        VisualLineKind::CodeLine { spans, .. } => {
            spans.iter().map(|span| span.content.as_ref()).collect()
        }
        VisualLineKind::BlockQuoteLine { spans, .. } => {
            spans.iter().map(|span| span.content.as_ref()).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "markdown")]
    use super::{DocumentFlattenCtx, flatten_blocks, spans_plain_text};
    use super::{DocumentVisualLine, VisualLineKind, build_visual_text_index};
    use crate::style::Span;
    #[cfg(feature = "markdown")]
    use crate::widgets::document_view::{
        DocumentStyles, DocumentTableWidthMode, FormatInput, MarkdownFormatter,
    };

    #[test]
    fn build_visual_text_index_tracks_offsets_and_byte_lengths() {
        let lines = vec![
            DocumentVisualLine {
                kind: VisualLineKind::Text {
                    spans: vec![Span::new("ab")],
                    indent_cols: 2,
                    continuation: false,
                    links: Vec::new(),
                },
                source_line: 0,
            },
            DocumentVisualLine {
                kind: VisualLineKind::HorizontalRule,
                source_line: 1,
            },
            DocumentVisualLine {
                kind: VisualLineKind::BlockQuoteLine {
                    spans: vec![Span::new("界")],
                    depth: 1,
                    links: Vec::new(),
                },
                source_line: 2,
            },
        ];

        let (line_texts, starts, lengths, flat) = build_visual_text_index(&lines, 2);

        let rendered: Vec<&str> = line_texts.iter().map(|s| s.as_ref()).collect();
        assert_eq!(rendered, vec!["  ab", "──", "界"]);
        assert_eq!(starts, vec![0, 5, 12]);
        assert_eq!(lengths, vec![4, 6, 3]);
        assert_eq!(flat.as_ref(), "  ab\n──\n界");
    }

    #[test]
    fn visual_line_plain_text_omits_code_block_left_inset() {
        use super::{
            CODE_BLOCK_LEFT_INSET_COLS, visual_line_plain_text, visual_line_render_prefix_cols,
        };

        let line = DocumentVisualLine {
            kind: VisualLineKind::CodeLine {
                spans: vec![Span::new("fn main()")],
                block_style: crate::style::Style::default(),
            },
            source_line: 0,
        };

        assert_eq!(visual_line_plain_text(&line, 0), "fn main()");
        assert_eq!(
            visual_line_render_prefix_cols(&line),
            CODE_BLOCK_LEFT_INSET_COLS as usize
        );
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn markdown_code_block_wraps_long_lines_when_wrap_enabled() {
        use super::{CODE_BLOCK_LEFT_INSET_COLS, DocumentFlattenCtx, flatten_blocks, span_width};
        use crate::style::BorderStyle;
        use crate::widgets::document_view::format::ContentFormatter;
        use crate::widgets::document_view::{
            DocumentStyles, DocumentTableWidthMode, FormatInput, MarkdownFormatter,
            TableRowSeparators,
        };

        let code =
            "Veritas MCP is a local-first research assistant used for my Master Thesis workflow.";
        let markdown = format!("```text\n{code}\n```");
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: &markdown,
            content_type: Some("markdown"),
            document_styles: None,
        });
        let doc_styles = DocumentStyles::default();
        let width = 24;
        let (lines, max_w) = flatten_blocks(
            &doc,
            width,
            DocumentFlattenCtx {
                wrap: true,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                doc_styles: &doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );

        let code_lines: Vec<(&Vec<Span>, usize)> = lines
            .iter()
            .filter_map(|line| match &line.kind {
                VisualLineKind::CodeLine { spans, .. } => Some((spans, line.source_line)),
                _ => None,
            })
            .collect();

        assert!(
            code_lines.len() > 1,
            "long fenced code line should wrap into multiple visual lines"
        );
        assert!(
            max_w <= width,
            "wrapped code max width should fit content width"
        );

        let first_source_line = code_lines[0].1;
        for (spans, source_line) in &code_lines {
            let rendered_width = span_width(spans).saturating_add(CODE_BLOCK_LEFT_INSET_COLS);
            assert!(
                rendered_width <= width,
                "wrapped code line width {rendered_width} should fit {width}"
            );
            assert_eq!(
                *source_line, first_source_line,
                "wrapped continuations should keep their source-line mapping"
            );
        }

        let reconstructed = code_lines
            .iter()
            .map(|(spans, _)| spans_plain_text(spans))
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(reconstructed, code);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn nested_list_code_block_flattens_with_list_indentation() {
        use crate::style::BorderStyle;
        use crate::widgets::document_view::TableRowSeparators;
        use crate::widgets::document_view::format::ContentFormatter;

        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "- item\n  ```text\n  nested code\n  ```",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let doc_styles = DocumentStyles::default();
        let (lines, max_w) = flatten_blocks(
            &doc,
            80,
            DocumentFlattenCtx {
                wrap: false,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                doc_styles: &doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );
        let code_text = lines
            .iter()
            .find_map(|line| match &line.kind {
                VisualLineKind::CodeLine { .. } => Some(super::visual_line_plain_text(line, 80)),
                _ => None,
            })
            .expect("code visual line");

        assert!(
            code_text.starts_with("  "),
            "code was flush-left: {code_text:?}"
        );
        assert!(code_text.ends_with("nested code"));
        assert!(max_w >= code_text.len() as u16);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn nested_list_code_block_wrap_budget_subtracts_indent_once() {
        use crate::style::BorderStyle;
        use crate::widgets::document_view::TableRowSeparators;
        use crate::widgets::document_view::format::ContentFormatter;

        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "- item\n  ```text\n  1234567\n  ```",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let doc_styles = DocumentStyles::default();
        let (lines, _) = flatten_blocks(
            &doc,
            10,
            DocumentFlattenCtx {
                wrap: true,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                doc_styles: &doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );
        let code_lines: Vec<String> = lines
            .iter()
            .filter_map(|line| match &line.kind {
                VisualLineKind::CodeLine { .. } => Some(super::visual_line_plain_text(line, 10)),
                _ => None,
            })
            .collect();

        assert_eq!(code_lines, vec!["  1234567"]);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn markdown_fenced_code_after_paragraph_gets_visual_spacer() {
        use super::{DocumentFlattenCtx, build_visual_text_index, flatten_blocks};
        use crate::app::input::text::paragraph_line_range;
        use crate::style::BorderStyle;
        use crate::widgets::document_view::format::ContentFormatter;
        use crate::widgets::document_view::{
            DocumentStyles, DocumentTableWidthMode, FormatInput, MarkdownFormatter,
            TableRowSeparators,
        };

        let command = "ln -s \"/media/user/DataDrive/Bottles/ARMGDDN-Games\" \
            \"/home/user/.var/app/com.usebottles.bottles/data/bottles/bottles/ARMGDDN-Games\"";
        let markdown = format!("**6. Create Symlinks**\n```bash\n{command}\n```");
        let doc_styles = DocumentStyles::default();

        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: &markdown,
            content_type: Some("markdown"),
            document_styles: None,
        });
        let (lines, _) = flatten_blocks(
            &doc,
            160,
            DocumentFlattenCtx {
                wrap: false,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                doc_styles: &doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );
        let (line_texts, _, _, flat_text) = build_visual_text_index(&lines, 160);
        let rendered: Vec<&str> = line_texts.iter().map(|line| line.as_ref()).collect();

        assert_eq!(rendered, vec!["6. Create Symlinks", "", command]);
        assert_eq!(
            flat_text.as_ref(),
            format!("6. Create Symlinks\n\n{command}")
        );
        assert_eq!(paragraph_line_range(&line_texts, 0), Some((0, 0)));
        assert_eq!(paragraph_line_range(&line_texts, 2), Some((2, 2)));

        let compact_formatter = MarkdownFormatter::default().compact_blocks(true);
        let compact_doc = compact_formatter.format(FormatInput {
            value: &markdown,
            content_type: Some("markdown"),
            document_styles: None,
        });
        let (compact_lines, _) = flatten_blocks(
            &compact_doc,
            160,
            DocumentFlattenCtx {
                wrap: false,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                doc_styles: &doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );
        let (_, _, _, compact_flat_text) = build_visual_text_index(&compact_lines, 160);

        assert_eq!(
            compact_flat_text.as_ref(),
            format!("6. Create Symlinks\n{command}")
        );
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn nested_markdown_list_sub_bullet_is_indented() {
        use super::{DocumentFlattenCtx, flatten_blocks};
        use crate::style::BorderStyle;
        use crate::widgets::document_view::format::ContentFormatter;
        use crate::widgets::document_view::{
            DocumentStyles, DocumentTableWidthMode, FormatInput, MarkdownFormatter,
            TableRowSeparators,
        };

        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: "- list\n  - sub",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let doc_styles = DocumentStyles::default();
        let (lines, _) = flatten_blocks(
            &doc,
            80,
            DocumentFlattenCtx {
                wrap: false,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                doc_styles: &doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );

        let indents: Vec<u16> = lines
            .iter()
            .filter_map(|l| match &l.kind {
                VisualLineKind::Text { indent_cols, .. } => Some(*indent_cols),
                _ => None,
            })
            .collect();

        assert_eq!(indents, vec![0, 2]);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn diagram_visual_text_index_uses_mermaid_source() {
        use super::{DocumentFlattenCtx, flatten_blocks};
        use crate::style::BorderStyle;
        use crate::widgets::document_view::{ContentFormatter, FormatInput, MarkdownFormatter};
        use crate::widgets::document_view::{
            DocumentStyles, DocumentTableWidthMode, TableRowSeparators,
        };

        let source = "flowchart TD\nA --> B";
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: &format!("```mermaid\n{source}\n```"),
            content_type: Some("markdown"),
            document_styles: None,
        });
        let (lines, _) = flatten_blocks(
            &doc,
            80,
            DocumentFlattenCtx {
                wrap: false,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                doc_styles: &DocumentStyles::default(),
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );
        let (_, _, _, flat_text) = build_visual_text_index(&lines, 80);

        assert_eq!(flat_text.as_ref(), source);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn gantt_mermaid_flattens_to_diagram_rows() {
        use super::{DocumentFlattenCtx, flatten_blocks};
        use crate::style::BorderStyle;
        use crate::widgets::document_view::{ContentFormatter, FormatInput, MarkdownFormatter};
        use crate::widgets::document_view::{
            DocumentStyles, DocumentTableWidthMode, TableRowSeparators,
        };

        let source = "gantt\n\
             title Sample Schedule\n\
             section Build\n\
             Design :a1, 2026-05-01, 3d\n\
             Release :milestone, after a1, 0d";
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: &format!("```mermaid\n{source}\n```"),
            content_type: Some("markdown"),
            document_styles: None,
        });
        let (lines, _) = flatten_blocks(
            &doc,
            80,
            DocumentFlattenCtx {
                wrap: false,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                doc_styles: &DocumentStyles::default(),
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );
        let diagram_text = lines
            .iter()
            .filter_map(|line| match &line.kind {
                VisualLineKind::DiagramRow { spans, .. } => Some(spans_plain_text(spans)),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let (_, _, _, flat_text) = build_visual_text_index(&lines, 80);

        assert!(diagram_text.contains("Sample Schedule"));
        assert!(diagram_text.contains("Build"));
        assert!(diagram_text.contains("Design"));
        assert!(diagram_text.contains('◆'));
        assert_eq!(flat_text.as_ref(), source);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn mermaid_style_directives_color_diagram_rows() {
        use super::{DocumentFlattenCtx, flatten_blocks};
        use crate::style::{BorderStyle, Color};
        use crate::widgets::document_view::{ContentFormatter, FormatInput, MarkdownFormatter};
        use crate::widgets::document_view::{
            DocumentStyles, DocumentTableWidthMode, TableRowSeparators,
        };

        let source = "graph TD\n\
             A[Client Request] --> B{API Gateway}\n\
             B -->|Authenticate| C[Auth Service]\n\
             B -->|Route| D[Load Balancer]\n\
             C -->|Invalid| E[401 Unauthorized]\n\
             F --> I[(Database)]\n\
             style A fill:#4CAF50,stroke:#333,color:#fff\n\
             style B fill:#2196F3,stroke:#333,color:#fff\n\
             style C fill:#FF9800,stroke:#333,color:#fff\n\
             style E fill:#F44336,stroke:#333,color:#fff\n\
             style I fill:#9C27B0,stroke:#333,color:#fff";
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: &format!("```mermaid\n{source}\n```"),
            content_type: Some("markdown"),
            document_styles: None,
        });
        let (lines, _) = flatten_blocks(
            &doc,
            120,
            DocumentFlattenCtx {
                wrap: false,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                doc_styles: &DocumentStyles::default(),
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );

        assert!(lines.iter().any(|line| {
            match &line.kind {
                VisualLineKind::DiagramRow { spans, .. } => spans
                    .iter()
                    .any(|span| span.style.bg == Some(Color::Rgb(0x4c, 0xaf, 0x50).into())),
                _ => false,
            }
        }));
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn diff_fenced_block_colors_added_and_removed_lines() {
        use super::{DocumentFlattenCtx, flatten_blocks};
        use crate::style::{BorderStyle, Color, Style};
        use crate::widgets::document_view::{ContentFormatter, FormatInput, MarkdownFormatter};
        use crate::widgets::document_view::{
            DocumentStyles, DocumentTableWidthMode, TableRowSeparators,
        };

        let mut doc_styles = DocumentStyles::default();
        let added_bg = Color::Rgb(0x00, 0x40, 0x00);
        let removed_bg = Color::Rgb(0x40, 0x00, 0x00);
        let header_fg = Color::Rgb(0x80, 0x80, 0xff);
        let added_marker_fg = Color::Rgb(0x00, 0xff, 0x00);
        let removed_marker_fg = Color::Rgb(0xff, 0x00, 0x00);
        doc_styles.diff_palette.added = Style::new().bg(added_bg);
        doc_styles.diff_palette.removed = Style::new().bg(removed_bg);
        doc_styles.diff_palette.added_marker = Style::new().fg(added_marker_fg);
        doc_styles.diff_palette.removed_marker = Style::new().fg(removed_marker_fg);
        doc_styles.diff_palette.patch_header = Style::new().fg(header_fg);

        let source = "@@ -1,3 +1,3 @@\n context\n-old line\n+new line\n more";
        let formatter = MarkdownFormatter::default();
        let doc = formatter.format(FormatInput {
            value: &format!("```diff\n{source}\n```"),
            content_type: Some("markdown"),
            document_styles: None,
        });
        let (lines, _) = flatten_blocks(
            &doc,
            80,
            DocumentFlattenCtx {
                wrap: false,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                doc_styles: &doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );

        let code_lines: Vec<(Vec<Span>, Style)> = lines
            .into_iter()
            .filter_map(|l| match l.kind {
                VisualLineKind::CodeLine { spans, block_style } => Some((spans, block_style)),
                _ => None,
            })
            .collect();
        assert_eq!(code_lines.len(), 5);

        // Hunk header line - patch_header style on span, no line bg
        assert!(code_lines[0].0[0].style.fg == Some(header_fg.into()));

        // Removed line gets removed bg
        assert_eq!(code_lines[2].1.bg, Some(removed_bg.into()));
        assert_eq!(code_lines[2].0[0].content.as_ref(), "-");

        // Added line gets added bg
        assert_eq!(code_lines[3].1.bg, Some(added_bg.into()));
        assert_eq!(code_lines[3].0[0].content.as_ref(), "+");
    }
}
