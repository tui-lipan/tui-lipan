//! Internal render-tree node for [`DocumentView`](super::DocumentView).

use std::rc::Rc;
use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::node::SecondaryPress;
use crate::core::node::{
    NodeKind, ScrollbarZone, ScrollbarZonesParams, WidgetNode, compute_scrollbar_zones,
};
use crate::style::{
    BorderStyle, Length, Padding, Rect, ScrollbarVariant, Span, Style, StyleSlot, Theme, ThemeRole,
};
use crate::widgets::context_menu::ContextMenuHit;

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
    CodeBlockHeader, CodeBlockPadding, DocumentClickEvent, DocumentLineNumberMode,
    DocumentSelectEvent, DocumentStyles, DocumentTableWidthMode, DocumentView, FormattedDocument,
    FormattedLine,
};

/// The visual-line kind after block flattening.
#[derive(Clone, Debug)]
pub(crate) enum VisualLineKind {
    /// A normal text line (possibly word-wrapped).
    Text {
        spans: Vec<Span>,
        indent_cols: u16,
        continuation: bool,
        links: Vec<VisualLink>,
        /// Source whitespace skipped at the wrap point that ends the previous row.
        /// It is not rendered, but copy restores it across the soft break.
        wrap_gap: Option<Arc<str>>,
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
        /// Leading container indentation (e.g. list nesting) painted with canvas background.
        indent_cols: u16,
        /// Blank code-background columns before the code (0 in one-column viewports).
        left_inset: u16,
        /// Target background rectangle width in columns (starting after `indent_cols`).
        rect_width: u16,
    },
    /// Non-selectable decorative row belonging to a fenced code block (vertical
    /// padding or language header).
    CodeBlockChrome {
        kind: CodeBlockChromeKind,
        block_style: Style,
        indent_cols: u16,
        left_inset: u16,
        rect_width: u16,
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
        links: Vec<VisualLink>,
        /// Source whitespace skipped at the wrap point that ends the previous row.
        wrap_gap: Option<Arc<str>>,
    },
}

/// Decorative row kind rendered around or atop a fenced code block.
#[derive(Clone, Debug)]
pub(crate) enum CodeBlockChromeKind {
    /// Top half-block cushion (`▄`) painted in the code block's background over canvas.
    TopHalfBlock,
    /// Bottom half-block cushion (`▀`) painted in the code block's background over canvas.
    BottomHalfBlock,
    /// Full blank padding row painted in the code block's background.
    BlankPadding,
    /// Language header row (icon and/or language label).
    Header {
        spans: Vec<Span>,
        /// The language name when the header shows only its icon, offered on hover.
        hint: Option<Arc<str>>,
    },
}

/// A link's range on one visual row, with what holds for the whole link it belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct VisualLink {
    /// Start byte offset (inclusive) in the row text.
    pub start: usize,
    /// End byte offset (exclusive) in the row text.
    pub end: usize,
    /// Link destination URL.
    pub url: Arc<str>,
    /// Where the whole link starts in its formatted line: the same on every row it wraps onto.
    pub origin: usize,
    /// Whether the whole link text, not just this row's part of it, already shows the URL.
    pub label_shows_url: bool,
}

/// Which part of a document a hint belongs to: two parts with the same text are still two parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HintPart {
    /// The header of the code block starting on this source line.
    CodeBlockHeader { source_line: usize },
    /// The link starting at `origin` in the formatted line of `source_line`.
    Link { source_line: usize, origin: usize },
}

/// Text a [`DocumentView`](super::DocumentView) offers for the part of it under the pointer,
/// shown by its [`tooltips`](super::DocumentView::tooltips).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DocumentHoverHint {
    /// The part the hint belongs to, stable while it scrolls or rewraps.
    pub part: HintPart,
    /// What the tooltip says.
    pub text: Arc<str>,
    /// The hinted part, relative to the document view's top-left corner.
    pub area: Rect,
}

/// Whether a link's visible text already says where it goes, so a tooltip would only repeat it.
fn link_label_shows_url(label: &str, url: &str) -> bool {
    let label = label.trim();
    label == url || url.strip_prefix("mailto:") == Some(label)
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
    /// Byte offset of the `\n` that follows visual row `index` in `flat_text`.
    ///
    /// `None` for rows that add no newline: code-block chrome, diagram rows
    /// continued on the next row, and the final content row.
    pub(crate) fn newline_after(&self, index: usize) -> Option<usize> {
        let line = self.lines.get(index)?;
        if matches!(line.kind, VisualLineKind::CodeBlockChrome { .. }) {
            return None;
        }
        let line_end = self
            .line_starts
            .get(index)?
            .saturating_add(*self.line_lengths.get(index)?);
        let next_start = *self.line_starts.get(index + 1)?;
        (next_start > line_end).then_some(line_end)
    }

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
    pub code_block_width: Length,
    pub code_block_padding: CodeBlockPadding,
    pub code_block_header: CodeBlockHeader,
    pub code_block_inset: u16,

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
    /// First ancestor `Frame` exposes a left/right edge an integrated vertical
    /// scrollbar can be drawn on. Resolved during reconcile (the node tree is
    /// not reachable from `From<DocumentView>` or the measure pass).
    pub parent_integrated_v: bool,
    /// As [`Self::parent_integrated_v`], for a top/bottom edge.
    pub parent_integrated_h: bool,
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
    pub on_context_menu: Option<Callback<(DocumentClickEvent, crate::core::event::MouseEvent)>>,
    pub on_select: Option<Callback<DocumentSelectEvent>>,
    pub on_key: Option<KeyHandler>,
    pub(crate) on_hover_hint: Option<Callback<Option<DocumentHoverHint>>>,
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
    #[cfg(feature = "diff-view")]
    pub diff_line_click: Option<crate::widgets::diff_view::DiffLineClickConfig>,
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
    fn secondary_hit_test_refinement(&self, press: SecondaryPress) -> Option<bool> {
        Some(self.context_menu_hit_at(press).is_some())
    }

    fn has_secondary_click(
        &self,
        button: crate::core::event::MouseButton,
        _mods: crate::core::event::KeyMods,
    ) -> bool {
        let secondary = self.on_context_menu.is_some();
        #[cfg(feature = "diff-view")]
        let secondary = secondary
            || self
                .diff_line_click
                .as_ref()
                .is_some_and(|config| config.on_context_menu.is_some());
        button == crate::core::event::MouseButton::Right && secondary
    }

    fn dispatch_context_menu(&self, press: SecondaryPress) -> bool {
        match self.context_menu_hit_at(press) {
            Some(ContextMenuHit::Widget(event)) => crate::widgets::context_menu::emit_context_menu(
                self.on_context_menu.as_ref(),
                press.mouse,
                || Some(event),
            ),
            #[cfg(feature = "diff-view")]
            Some(ContextMenuHit::DiffSource(source)) => self
                .diff_line_click
                .as_ref()
                .is_some_and(|config| config.dispatch_context_menu(source, press.mouse)),
            None => false,
        }
    }

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
            || self.has_diff_line_click()
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
            || self.on_hover_hint.is_some()
            || self.has_diff_context_separator_hover()
            || self.hover_style.has_explicit_style()
            || self.hover_border_style.is_some()
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        self.on_click.is_some()
            || self.on_hover_hint.is_some()
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
    #[cfg(feature = "diff-view")]
    fn diff_context_menu_source_at(&self, press: SecondaryPress) -> Option<usize> {
        let config = self.diff_line_click.as_ref()?;
        let inner = press.rect.inner(self.border, self.padding);
        let cl = self.content_layout(inner);
        let row_rect = Rect {
            x: inner.x,
            y: cl.content_y,
            w: cl
                .content_width
                .saturating_add(cl.content_x.saturating_sub(inner.x) as u16),
            h: cl.content_height,
        };
        if !row_rect.contains(press.mouse.x as i16, press.mouse.y as i16) {
            return None;
        }
        let visual_idx = self
            .scroll_offset
            .saturating_add((press.mouse.y as i16 - inner.y) as usize);
        let line = self.visual_cache.lines.get(visual_idx)?;
        if matches!(&line.kind, VisualLineKind::Text { spans, continuation: true, .. } if spans.is_empty())
        {
            return None;
        }
        config.context_menu_event_at(line.source_line, press.mouse.mods)?;
        Some(line.source_line)
    }

    fn context_menu_hit_at(
        &self,
        press: SecondaryPress,
    ) -> Option<ContextMenuHit<DocumentClickEvent>> {
        #[cfg(feature = "diff-view")]
        if let Some(source) = self.diff_context_menu_source_at(press) {
            return Some(ContextMenuHit::DiffSource(source));
        }
        self.on_context_menu.as_ref()?;
        self.click_event_at(press.rect, press.mouse.x as i16, press.mouse.y as i16)
            .map(ContextMenuHit::Widget)
    }

    pub(crate) fn click_event_at(&self, rect: Rect, x: i16, y: i16) -> Option<DocumentClickEvent> {
        let inner = rect.inner(self.border, self.padding);
        let cl = self.content_layout(inner);
        let content = Rect {
            x: cl.content_x,
            y: cl.content_y,
            w: cl.content_width,
            h: cl.content_height,
        };
        if !content.contains(x, y) {
            return None;
        }
        let visual_idx = self
            .scroll_offset
            .saturating_add(y.saturating_sub(inner.y) as usize);
        let source_line = *self.visual_cache.source_line_map.get(visual_idx)?;
        let rel_x = x.saturating_sub(cl.content_x) as usize;
        let visual_col = if self.wrap {
            rel_x
        } else {
            rel_x.saturating_add(self.h_scroll_offset)
        };
        let link = self.visual_cache.lines.get(visual_idx).and_then(|vline| {
            let text = self.visual_cache.line_texts.get(visual_idx)?;
            let byte = document_view_byte_in_line_from_visual_col(text.as_ref(), vline, visual_col);
            let links = match &vline.kind {
                VisualLineKind::Text { links, .. }
                | VisualLineKind::BlockQuoteLine { links, .. } => links.as_slice(),
                _ => &[],
            };
            links
                .iter()
                .find(|link| byte >= link.start && byte < link.end)
                .map(|link| link.url.clone())
        });
        Some(DocumentClickEvent { source_line, link })
    }

    /// The hint under the pointer at `(x, y)`: the language of an icon-only code-block header, or
    /// the URL of a link whose text does not already show it.
    pub(crate) fn hover_hint_at(&self, rect: Rect, x: i16, y: i16) -> Option<DocumentHoverHint> {
        let inner = rect.inner(self.border, self.padding);
        let cl = self.content_layout(inner);
        let content = Rect {
            x: cl.content_x,
            y: cl.content_y,
            w: cl.content_width,
            h: cl.content_height,
        };
        if !content.contains(x, y) {
            return None;
        }
        let visual_idx = self
            .scroll_offset
            .saturating_add(y.saturating_sub(inner.y) as usize);
        let vline = self.visual_cache.lines.get(visual_idx)?;
        let h_scroll = if self.wrap { 0 } else { self.h_scroll_offset };
        let visual_col = (x.saturating_sub(cl.content_x) as usize).saturating_add(h_scroll);
        let (part, text, start_col, end_col) = match &vline.kind {
            VisualLineKind::CodeBlockChrome {
                kind:
                    CodeBlockChromeKind::Header {
                        spans,
                        hint: Some(hint),
                    },
                indent_cols,
                left_inset,
                ..
            } => {
                let start = usize::from(indent_cols.saturating_add(*left_inset));
                (
                    HintPart::CodeBlockHeader {
                        source_line: vline.source_line,
                    },
                    hint.clone(),
                    start,
                    start.saturating_add(span_width(spans) as usize),
                )
            }
            VisualLineKind::Text { links, .. } | VisualLineKind::BlockQuoteLine { links, .. } => {
                let line_text = self.visual_cache.line_texts.get(visual_idx)?;
                let byte = document_view_byte_in_line_from_visual_col(line_text, vline, visual_col);
                let link = links
                    .iter()
                    .find(|link| byte >= link.start && byte < link.end)
                    .filter(|link| !link.label_shows_url)?;
                let start = link.start.min(line_text.len());
                let end = link.end.min(line_text.len());
                let prefix = visual_line_render_prefix_cols(vline);
                let col = |byte: usize| {
                    prefix.saturating_add(crate::utils::text::str_visual_width(
                        line_text.get(..byte).unwrap_or_default(),
                        None,
                    ))
                };
                (
                    HintPart::Link {
                        source_line: vline.source_line,
                        origin: link.origin,
                    },
                    link.url.clone(),
                    col(start),
                    col(end),
                )
            }
            _ => return None,
        };
        if visual_col < start_col || visual_col >= end_col {
            return None;
        }
        let left = (cl.content_x as i32)
            .saturating_add(start_col as i32)
            .saturating_sub(h_scroll as i32);
        let right = (cl.content_x as i32)
            .saturating_add(end_col as i32)
            .saturating_sub(h_scroll as i32);
        let left = left.max(i32::from(content.x));
        let right = right.min(i32::from(content.x) + i32::from(content.w));
        Some(DocumentHoverHint {
            part,
            text,
            area: Rect {
                x: (left - i32::from(rect.x)) as i16,
                y: y.saturating_sub(rect.y),
                w: right.saturating_sub(left).max(0) as u16,
                h: 1,
            },
        })
    }

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

    fn has_diff_line_click(&self) -> bool {
        #[cfg(feature = "diff-view")]
        {
            self.diff_line_click
                .as_ref()
                .is_some_and(|config| config.on_click.is_some() || config.on_range_select.is_some())
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
            && (self.border || self.parent_integrated_v);
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
            && (self.border || self.parent_integrated_h);
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
            code_block_width: dv.code_block_width,
            code_block_padding: dv.code_block_padding,
            code_block_header: dv.code_block_header,
            code_block_inset: dv.code_block_inset,
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
            // Needs the node tree; filled in by `reconcile_document_view`.
            parent_integrated_v: false,
            parent_integrated_h: false,
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
            on_context_menu: dv.on_context_menu,
            on_select: dv.on_select,
            on_key: dv.on_key,
            on_hover_hint: dv.on_hover_hint,
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
            #[cfg(feature = "diff-view")]
            diff_line_click: dv.diff_line_click,
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
    pub code_block_width: Length,
    pub code_block_padding: CodeBlockPadding,
    pub code_block_header: CodeBlockHeader,
    pub code_block_inset: u16,
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

/// Display columns rendered before selectable content (code inset, blockquote bars).
pub(crate) fn visual_line_render_prefix_cols(vline: &DocumentVisualLine) -> usize {
    match &vline.kind {
        VisualLineKind::CodeLine {
            indent_cols,
            left_inset,
            ..
        } => (*indent_cols as usize).saturating_add(*left_inset as usize),
        VisualLineKind::CodeBlockChrome { .. } => usize::MAX,
        VisualLineKind::BlockQuoteLine { depth, .. } => (*depth as usize).saturating_mul(2),
        _ => 0,
    }
}

/// UTF-8 bytes in the render-only prefix for a visual line.
pub(crate) fn visual_line_render_prefix_bytes(vline: &DocumentVisualLine) -> usize {
    match &vline.kind {
        VisualLineKind::CodeLine {
            indent_cols,
            left_inset,
            ..
        } => (*indent_cols as usize).saturating_add(*left_inset as usize),
        VisualLineKind::CodeBlockChrome { .. } => usize::MAX,
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
        | VisualLineKind::DiagramRow { spans, .. }
        | VisualLineKind::CodeBlockChrome {
            kind: CodeBlockChromeKind::Header { spans, .. },
            ..
        } => {
            merge_spans(spans);
        }
        VisualLineKind::TableRow { cells, .. } => {
            for cell in cells.iter_mut() {
                merge_spans(cell);
            }
        }
        VisualLineKind::TableBorder { .. }
        | VisualLineKind::HorizontalRule
        | VisualLineKind::CodeBlockChrome { .. } => {}
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

fn flatten_text_lines(
    formatted_lines: &[super::FormattedLine],
    inner_w: u16,
    ctx: &DocumentFlattenCtx<'_>,
    accum: &mut DocumentFlattenAccum<'_>,
) {
    let wrap = ctx.wrap;
    let bq_depth = accum.bq_depth;
    let base_indent_cols = accum.base_indent_cols;
    for fl in formatted_lines {
        let indent_cols = base_indent_cols.saturating_add(fl.indent * 2);
        let line_w = span_width(&fl.spans);
        *accum.max_w = (*accum.max_w).max(line_w + indent_cols);

        if wrap && line_w + indent_cols > inner_w {
            // Word-wrap this line
            let avail = inner_w.saturating_sub(indent_cols);
            let wrapped = wrap_spans_with_links(&fl.spans, &visual_links(fl), avail, avail);
            let anchor = extract_bg_anchor(&fl.spans);
            for (i, (mut wline, links, wrap_gap)) in wrapped.into_iter().enumerate() {
                if let Some(ref anchor) = anchor {
                    wline.insert(0, anchor.clone());
                }
                accum.out.push(DocumentVisualLine {
                    kind: if bq_depth > 0 {
                        VisualLineKind::BlockQuoteLine {
                            spans: wline,
                            depth: bq_depth,
                            links,
                            wrap_gap,
                        }
                    } else {
                        VisualLineKind::Text {
                            spans: wline,
                            indent_cols,
                            continuation: i > 0,
                            links: shift_links(&links, indent_cols as usize),
                            wrap_gap,
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
                        links: visual_links(fl),
                        wrap_gap: None,
                    }
                } else {
                    let indent_bytes = indent_cols as usize;
                    VisualLineKind::Text {
                        spans: fl.spans.clone(),
                        indent_cols,
                        continuation: false,
                        links: shift_links(&visual_links(fl), indent_bytes),
                        wrap_gap: None,
                    }
                },
                source_line: fl.source_line,
            });
        }
    }
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
            flatten_text_lines(formatted_lines, inner_w, ctx, accum);
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
                    code_block_width: ctx.code_block_width,
                    code_block_padding: ctx.code_block_padding,
                    code_block_header: ctx.code_block_header,
                    code_block_inset: ctx.code_block_inset,
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
                                        for (mut wline, links, wrap_gap) in wrap_spans_with_links(
                                            &spans,
                                            &shift_links(&visual_links(fl), line_prefix_bytes),
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
                                                    links,
                                                    wrap_gap,
                                                },
                                                source_line: fl.source_line,
                                            });
                                        }
                                    } else {
                                        accum.out.push(DocumentVisualLine {
                                            kind: VisualLineKind::BlockQuoteLine {
                                                spans,
                                                depth: bq_depth,
                                                links: shift_links(
                                                    &visual_links(fl),
                                                    line_prefix_bytes,
                                                ),
                                                wrap_gap: None,
                                            },
                                            source_line: fl.source_line,
                                        });
                                    }
                                } else if should_wrap {
                                    let wrapped = wrap_spans_with_links(
                                        &spans,
                                        &shift_links(
                                            &visual_links(fl),
                                            if li == 0 { prefix.len() } else { 0 },
                                        ),
                                        first_budget,
                                        cont_budget,
                                    );
                                    let anchor = extract_bg_anchor(&fl.spans);
                                    for (wi, (mut wline, links, wrap_gap)) in
                                        wrapped.into_iter().enumerate()
                                    {
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
                                                links: shift_links(&links, w_indent_cols as usize),
                                                wrap_gap,
                                            },
                                            source_line: fl.source_line,
                                        });
                                    }
                                } else {
                                    let links = if li == 0 {
                                        shift_links(&visual_links(fl), prefix.len())
                                    } else {
                                        visual_links(fl)
                                    };
                                    accum.out.push(DocumentVisualLine {
                                        kind: VisualLineKind::Text {
                                            spans,
                                            indent_cols,
                                            continuation: li > 0,
                                            links,
                                            wrap_gap: None,
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
    code_block_width: Length,
    code_block_padding: CodeBlockPadding,
    code_block_header: CodeBlockHeader,
    code_block_inset: u16,
    doc_styles: &'a DocumentStyles,
    base_indent_cols: u16,
    #[cfg(feature = "syntax-syntect")]
    code_strategy: Option<&'a dyn crate::widgets::TextAreaColorStrategy>,
}

/// The header row's spans, and the language name when only its icon is shown.
fn code_block_header_spans(
    language: Option<&str>,
    header: CodeBlockHeader,
    doc_styles: &DocumentStyles,
) -> Option<(Vec<Span>, Option<Arc<str>>)> {
    if header == CodeBlockHeader::None {
        return None;
    }
    let label = language?.trim();
    if label.is_empty() {
        return None;
    }
    let plain = || Some((vec![Span::new(label.to_string())], None));
    match header {
        CodeBlockHeader::None => None,
        CodeBlockHeader::Label => plain(),
        CodeBlockHeader::Icon | CodeBlockHeader::IconAndLabel => {
            let Some(icon) = crate::utils::language_icon_span(label, &doc_styles.file_icons) else {
                return plain();
            };
            if header == CodeBlockHeader::Icon {
                Some((vec![icon], Some(Arc::from(label))))
            } else {
                Some((vec![icon, Span::new(format!(" {label}"))], None))
            }
        }
    }
}

fn flatten_code_block(
    cb: &super::FormattedCodeBlock,
    ctx: CodeFlattenCtx<'_>,
    out: &mut Vec<DocumentVisualLine>,
    max_w: &mut u16,
) {
    let base_block_style = ctx.doc_styles.code_block_style;
    let is_diff = cb.language.as_deref().is_some_and(|lang| {
        lang.eq_ignore_ascii_case("diff") || lang.eq_ignore_ascii_case("patch")
    });

    let raw_lines: Vec<(Vec<Span>, Style, usize)> = if is_diff {
        let palette = &ctx.doc_styles.diff_palette;
        cb.code
            .split('\n')
            .enumerate()
            .map(|(i, line_text)| {
                let (line_bg_style, spans) = classify_diff_line(line_text, palette);
                (
                    spans,
                    base_block_style.patch(line_bg_style),
                    cb.source_line_start.saturating_add(i),
                )
            })
            .collect()
    } else {
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

        cb.code
            .split('\n')
            .enumerate()
            .map(|(i, line_text)| {
                let spans = if let Some(ref hl_lines) = highlighted {
                    hl_lines
                        .get(i)
                        .cloned()
                        .unwrap_or_else(|| vec![Span::new(line_text)])
                } else {
                    vec![Span::new(line_text)]
                };
                (
                    spans,
                    base_block_style,
                    cb.source_line_start.saturating_add(i),
                )
            })
            .collect()
    };

    let header_spans = code_block_header_spans(
        cb.language.as_deref(),
        ctx.code_block_header,
        ctx.doc_styles,
    );

    let mut left_inset = ctx.code_block_inset;
    // Decorated rectangles mirror the inset on the right so code never touches
    // the visible right edge; plain flex blocks run to the edge.
    let decorated = ctx.code_block_padding != CodeBlockPadding::None
        || ctx.code_block_header != CodeBlockHeader::None
        || !matches!(ctx.code_block_width, Length::Flex(_));
    let mut right_inset = if decorated { left_inset } else { 0 };
    let available_w = ctx.inner_w.saturating_sub(ctx.base_indent_cols);
    // Content columns the insets must leave: wrapping cannot split a grapheme,
    // so the widest one sets the floor.
    let min_content_w = if ctx.wrap {
        raw_lines
            .iter()
            .map(|(spans, _, _)| crate::utils::text::widest_grapheme_width(spans))
            .max()
            .unwrap_or(0)
            .max(1)
    } else {
        1
    };

    let header_content_w = header_spans
        .as_ref()
        .map_or(0, |(spans, _)| span_width(spans));
    let max_raw_code_w = raw_lines
        .iter()
        .map(|(spans, _, _)| span_width(spans))
        .max()
        .unwrap_or(0);
    let natural_rect_w = max_raw_code_w
        .max(header_content_w)
        .saturating_add(left_inset.saturating_add(right_inset));

    // `Px`/`Percent` are fixed widths while wrapping; with wrapping off nothing
    // can shrink the code, so they act as minimum widths instead. The insets
    // fit inside the resolved width, never widen it.
    let target_rect_w = match ctx.code_block_width {
        Length::Flex(_) => {
            if ctx.wrap {
                available_w
            } else {
                available_w.max(natural_rect_w)
            }
        }
        Length::Auto => {
            if ctx.wrap {
                natural_rect_w.min(available_w)
            } else {
                natural_rect_w
            }
        }
        Length::Px(px) => {
            let px = px.max(1);
            if ctx.wrap {
                px.min(available_w)
            } else {
                px.max(natural_rect_w)
            }
        }
        Length::Percent(pct) => {
            let scaled = ((u32::from(available_w) * u32::from(pct.min(100))) / 100) as u16;
            let scaled = scaled.max(1).min(available_w);
            if ctx.wrap {
                scaled
            } else {
                scaled.max(natural_rect_w)
            }
        }
    };

    // Too narrow for the insets: shrink them to leave `min_content_w` content
    // columns, right side first.
    let inset_room = target_rect_w.saturating_sub(min_content_w);
    right_inset = right_inset.min(inset_room.saturating_sub(left_inset));
    left_inset = left_inset.min(inset_room);
    let h_insets = left_inset.saturating_add(right_inset);
    let wrap_budget = target_rect_w.saturating_sub(h_insets).max(1);

    let header_spans = header_spans.map(|(spans, hint)| {
        if ctx.wrap && header_content_w > wrap_budget {
            (
                crate::utils::spans::truncate_spans(&spans, wrap_budget as usize),
                hint,
            )
        } else {
            (spans, hint)
        }
    });
    let header_content_w = header_spans
        .as_ref()
        .map_or(0, |(spans, _)| span_width(spans));

    let mut rendered_rows: Vec<(Vec<Span>, Style, usize)> = Vec::with_capacity(raw_lines.len());
    let mut actual_max_content_w = header_content_w;

    for (spans, block_style, source_line) in raw_lines {
        let code_w = span_width(&spans);
        if ctx.wrap && code_w > wrap_budget {
            for wrapped in crate::utils::text::wrap_preformatted_spans_for_budgets(
                &spans,
                wrap_budget,
                wrap_budget,
            ) {
                actual_max_content_w = actual_max_content_w.max(span_width(&wrapped));
                rendered_rows.push((wrapped, block_style, source_line));
            }
        } else {
            actual_max_content_w = actual_max_content_w.max(code_w);
            rendered_rows.push((spans, block_style, source_line));
        }
    }

    let content_rect_w = actual_max_content_w.saturating_add(h_insets);
    let rect_width = match ctx.code_block_width {
        Length::Auto => {
            if ctx.wrap {
                content_rect_w.min(available_w)
            } else {
                content_rect_w
            }
        }
        // A wide glyph can exceed a one-column budget; grow so it stays inside.
        _ => target_rect_w.max(actual_max_content_w.saturating_add(left_inset)),
    };

    let reported_w = match ctx.code_block_width {
        Length::Flex(_) => ctx.base_indent_cols.saturating_add(content_rect_w),
        _ => ctx
            .base_indent_cols
            .saturating_add(rect_width.max(content_rect_w)),
    };
    *max_w = (*max_w).max(reported_w);

    let first_source_line = cb.source_line_start;
    let last_source_line = rendered_rows
        .last()
        .map_or(first_source_line, |(_, _, line)| *line);

    match ctx.code_block_padding {
        CodeBlockPadding::None => {}
        CodeBlockPadding::HalfBlock => {
            out.push(DocumentVisualLine {
                kind: VisualLineKind::CodeBlockChrome {
                    kind: CodeBlockChromeKind::TopHalfBlock,
                    block_style: base_block_style,
                    indent_cols: ctx.base_indent_cols,
                    left_inset,
                    rect_width,
                },
                source_line: first_source_line,
            });
        }
        CodeBlockPadding::Full => {
            out.push(DocumentVisualLine {
                kind: VisualLineKind::CodeBlockChrome {
                    kind: CodeBlockChromeKind::BlankPadding,
                    block_style: base_block_style,
                    indent_cols: ctx.base_indent_cols,
                    left_inset,
                    rect_width,
                },
                source_line: first_source_line,
            });
        }
    }

    if let Some((spans, hint)) = header_spans {
        out.push(DocumentVisualLine {
            kind: VisualLineKind::CodeBlockChrome {
                kind: CodeBlockChromeKind::Header { spans, hint },
                block_style: base_block_style,
                indent_cols: ctx.base_indent_cols,
                left_inset,
                rect_width,
            },
            source_line: first_source_line,
        });
    }

    for (spans, block_style, source_line) in rendered_rows {
        out.push(DocumentVisualLine {
            kind: VisualLineKind::CodeLine {
                spans,
                block_style,
                indent_cols: ctx.base_indent_cols,
                left_inset,
                rect_width,
            },
            source_line,
        });
    }

    match ctx.code_block_padding {
        CodeBlockPadding::None => {}
        CodeBlockPadding::HalfBlock => {
            out.push(DocumentVisualLine {
                kind: VisualLineKind::CodeBlockChrome {
                    kind: CodeBlockChromeKind::BottomHalfBlock,
                    block_style: base_block_style,
                    indent_cols: ctx.base_indent_cols,
                    left_inset,
                    rect_width,
                },
                source_line: last_source_line,
            });
        }
        CodeBlockPadding::Full => {
            out.push(DocumentVisualLine {
                kind: VisualLineKind::CodeBlockChrome {
                    kind: CodeBlockChromeKind::BlankPadding,
                    block_style: base_block_style,
                    indent_cols: ctx.base_indent_cols,
                    left_inset,
                    rect_width,
                },
                source_line: last_source_line,
            });
        }
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

/// One wrapped row: its spans, its link ranges, and the source whitespace
/// skipped at the wrap point before it.
type WrappedRow = (Vec<Span>, Vec<VisualLink>, Option<Arc<str>>);

/// Preserve semantic link ranges through wrapping, including a link spanning several rows.
fn wrap_spans_with_links(
    spans: &[Span],
    links: &[VisualLink],
    first_budget: u16,
    cont_budget: u16,
) -> Vec<WrappedRow> {
    let mut span_offsets = Vec::with_capacity(spans.len());
    let mut offset = 0usize;
    for span in spans {
        span_offsets.push(offset);
        offset += span.content.len();
    }
    let flat_offset = |cursor: crate::utils::text::SpanCursor| {
        span_offsets
            .get(cursor.span_idx)
            .map_or(offset, |start| start + cursor.byte_idx)
    };

    let mut prev_end = None;
    crate::utils::text::wrap_span_ranges_for_budgets(spans, first_budget, cont_budget)
        .into_iter()
        .map(|range| {
            let line = crate::utils::text::collect_span_range(spans, range.start, range.end);
            let wrap_gap = prev_end
                .replace(range.end)
                .filter(|prev| flat_offset(*prev) < flat_offset(range.start))
                .map(|prev| {
                    Arc::<str>::from(spans_plain_text(&crate::utils::text::collect_span_range(
                        spans,
                        prev,
                        range.start,
                    )))
                });
            let start = flat_offset(range.start);
            let end = flat_offset(range.end);
            let row_links = links
                .iter()
                .filter_map(|link| {
                    let first = start.max(link.start);
                    let last = end.min(link.end);
                    (first < last).then(|| VisualLink {
                        start: first - start,
                        end: last - start,
                        ..link.clone()
                    })
                })
                .collect();
            (line, row_links, wrap_gap)
        })
        .collect()
}

fn shift_links(links: &[VisualLink], shift_bytes: usize) -> Vec<VisualLink> {
    links
        .iter()
        .map(|link| VisualLink {
            start: link.start.saturating_add(shift_bytes),
            end: link.end.saturating_add(shift_bytes),
            ..link.clone()
        })
        .collect()
}

/// A formatted line's links, each knowing its whole extent before wrapping splits it into rows.
fn visual_links(fl: &FormattedLine) -> Vec<VisualLink> {
    if fl.links.is_empty() {
        return Vec::new();
    }
    let text = spans_plain_text(&fl.spans);
    fl.links
        .iter()
        .map(|link| VisualLink {
            start: link.start,
            end: link.end,
            url: link.url.clone(),
            origin: link.start,
            label_shows_url: text
                .get(link.start..link.end)
                .is_some_and(|label| link_label_shows_url(label, &link.url)),
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
        let is_code_chrome = matches!(line.kind, VisualLineKind::CodeBlockChrome { .. });
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
        if !is_diagram_continuation && !is_code_chrome {
            flat.push_str(&text);
        }
        let next_is_diagram_continuation = lines.get(i + 1).is_some_and(|next| {
            is_diagram_row
                && next.source_line == line.source_line
                && matches!(next.kind, VisualLineKind::DiagramRow { .. })
        });
        let has_later_non_chrome = lines.get(i + 1..).is_some_and(|rest| {
            rest.iter()
                .any(|later| !matches!(later.kind, VisualLineKind::CodeBlockChrome { .. }))
        });
        if !is_code_chrome && !next_is_diagram_continuation && has_later_non_chrome {
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
                    wrap_gap: None,
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
        VisualLineKind::CodeBlockChrome { .. } => String::new(),
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
                    wrap_gap: None,
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
                    wrap_gap: None,
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
        use super::{visual_line_plain_text, visual_line_render_prefix_cols};

        let line = DocumentVisualLine {
            kind: VisualLineKind::CodeLine {
                spans: vec![Span::new("fn main()")],
                block_style: crate::style::Style::default(),
                indent_cols: 0,
                left_inset: 1,
                rect_width: 0,
            },
            source_line: 0,
        };

        assert_eq!(visual_line_plain_text(&line, 0), "fn main()");
        assert_eq!(visual_line_render_prefix_cols(&line), 1);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn markdown_paragraph_wrap_keeps_the_space_on_the_row_it_ends() {
        use crate::style::BorderStyle;
        use crate::widgets::document_view::TableRowSeparators;
        use crate::widgets::document_view::format::ContentFormatter;

        let doc = MarkdownFormatter::default().format(FormatInput {
            value: "safely. Runs [link](https://example.com)",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let doc_styles = DocumentStyles::default();
        let (lines, _) = flatten_blocks(
            &doc,
            7,
            DocumentFlattenCtx {
                wrap: true,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                code_block_width: crate::style::Length::Flex(1),
                code_block_padding: crate::widgets::document_view::CodeBlockPadding::None,
                code_block_header: crate::widgets::document_view::CodeBlockHeader::None,
                code_block_inset: 1,
                doc_styles: &doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );

        let rows: Vec<(String, Vec<(usize, usize)>)> = lines
            .iter()
            .filter_map(|line| match &line.kind {
                VisualLineKind::Text { spans, links, .. } => Some((
                    spans_plain_text(spans),
                    links.iter().map(|link| (link.start, link.end)).collect(),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("safely.".to_string(), vec![]),
                ("Runs ".to_string(), vec![]),
                ("link".to_string(), vec![(0, 4)]),
            ]
        );
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn markdown_code_block_wraps_long_lines_when_wrap_enabled() {
        use super::{DocumentFlattenCtx, flatten_blocks, span_width};
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
                code_block_width: crate::style::Length::Flex(1),
                code_block_padding: crate::widgets::document_view::CodeBlockPadding::None,
                code_block_header: crate::widgets::document_view::CodeBlockHeader::None,
                code_block_inset: 1,
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
            let rendered_width = span_width(spans).saturating_add(1);
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

    #[cfg(all(feature = "markdown", feature = "syntax-extra"))]
    #[test]
    fn markdown_toml_fence_reaches_document_code_highlighter() {
        use super::{DocumentFlattenCtx, VisualLineKind, flatten_blocks};
        use crate::style::BorderStyle;
        use crate::widgets::SyntectStrategy;
        use crate::widgets::document_view::format::ContentFormatter;
        use crate::widgets::document_view::{
            DocumentStyles, DocumentTableWidthMode, FormatInput, MarkdownFormatter,
            TableRowSeparators,
        };

        let markdown = "```toml\nname = \"tui-lipan\"\n```";
        let doc = MarkdownFormatter::default().format(FormatInput {
            value: markdown,
            content_type: Some("markdown"),
            document_styles: None,
        });
        let doc_styles = DocumentStyles::default();
        let strategy = SyntectStrategy::default();
        let (lines, _) = flatten_blocks(
            &doc,
            80,
            DocumentFlattenCtx {
                wrap: true,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                code_block_width: crate::style::Length::Flex(1),
                code_block_padding: crate::widgets::document_view::CodeBlockPadding::None,
                code_block_header: crate::widgets::document_view::CodeBlockHeader::None,
                code_block_inset: 1,
                doc_styles: &doc_styles,
                code_strategy: Some(&strategy),
            },
        );

        let code_line = lines
            .iter()
            .find_map(|line| match &line.kind {
                VisualLineKind::CodeLine { spans, .. }
                    if spans
                        .iter()
                        .any(|span| span.content.as_ref().contains("name")) =>
                {
                    Some(spans)
                }
                _ => None,
            })
            .expect("TOML code line");
        assert!(code_line.iter().any(|span| span.style.fg.is_some()));
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
                code_block_width: crate::style::Length::Flex(1),
                code_block_padding: crate::widgets::document_view::CodeBlockPadding::None,
                code_block_header: crate::widgets::document_view::CodeBlockHeader::None,
                code_block_inset: 1,
                doc_styles: &doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );
        let (indent_cols, code_text) = lines
            .iter()
            .find_map(|line| match &line.kind {
                VisualLineKind::CodeLine { indent_cols, .. } => {
                    Some((*indent_cols, super::visual_line_plain_text(line, 80)))
                }
                _ => None,
            })
            .expect("code visual line");

        assert_eq!(indent_cols, 2, "code should carry list indentation");
        assert_eq!(code_text, "nested code");
        assert!(max_w >= (indent_cols as usize + code_text.len()) as u16);
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
                code_block_width: crate::style::Length::Flex(1),
                code_block_padding: crate::widgets::document_view::CodeBlockPadding::None,
                code_block_header: crate::widgets::document_view::CodeBlockHeader::None,
                code_block_inset: 1,
                doc_styles: &doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );
        let code_lines: Vec<(u16, String)> = lines
            .iter()
            .filter_map(|line| match &line.kind {
                VisualLineKind::CodeLine { indent_cols, .. } => {
                    Some((*indent_cols, super::visual_line_plain_text(line, 10)))
                }
                _ => None,
            })
            .collect();

        assert_eq!(code_lines, vec![(2, "1234567".to_string())]);
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
                code_block_width: crate::style::Length::Flex(1),
                code_block_padding: crate::widgets::document_view::CodeBlockPadding::None,
                code_block_header: crate::widgets::document_view::CodeBlockHeader::None,
                code_block_inset: 1,
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

        let compact_formatter = MarkdownFormatter::default()
            .spacing(crate::widgets::document_view::MarkdownSpacing::COMPACT);
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
                code_block_width: crate::style::Length::Flex(1),
                code_block_padding: crate::widgets::document_view::CodeBlockPadding::None,
                code_block_header: crate::widgets::document_view::CodeBlockHeader::None,
                code_block_inset: 1,
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
                code_block_width: crate::style::Length::Flex(1),
                code_block_padding: crate::widgets::document_view::CodeBlockPadding::None,
                code_block_header: crate::widgets::document_view::CodeBlockHeader::None,
                code_block_inset: 1,
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
                code_block_width: crate::style::Length::Flex(1),
                code_block_padding: crate::widgets::document_view::CodeBlockPadding::None,
                code_block_header: crate::widgets::document_view::CodeBlockHeader::None,
                code_block_inset: 1,
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
                code_block_width: crate::style::Length::Flex(1),
                code_block_padding: crate::widgets::document_view::CodeBlockPadding::None,
                code_block_header: crate::widgets::document_view::CodeBlockHeader::None,
                code_block_inset: 1,
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
                code_block_width: crate::style::Length::Flex(1),
                code_block_padding: crate::widgets::document_view::CodeBlockPadding::None,
                code_block_header: crate::widgets::document_view::CodeBlockHeader::None,
                code_block_inset: 1,
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
                code_block_width: crate::style::Length::Flex(1),
                code_block_padding: crate::widgets::document_view::CodeBlockPadding::None,
                code_block_header: crate::widgets::document_view::CodeBlockHeader::None,
                code_block_inset: 1,
                doc_styles: &doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );

        let code_lines: Vec<(Vec<Span>, Style)> = lines
            .into_iter()
            .filter_map(|l| match l.kind {
                VisualLineKind::CodeLine {
                    spans, block_style, ..
                } => Some((spans, block_style)),
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

    #[cfg(feature = "markdown")]
    #[test]
    fn code_block_width_modes_compute_expected_rect_widths() {
        use super::{CodeBlockChromeKind, DocumentFlattenCtx, VisualLineKind, flatten_blocks};
        use crate::style::{BorderStyle, Length};
        use crate::widgets::document_view::{
            CodeBlockHeader, CodeBlockPadding, ContentFormatter, DocumentStyles,
            DocumentTableWidthMode, FormatInput, MarkdownFormatter, TableRowSeparators,
        };

        let doc = MarkdownFormatter::default().format(FormatInput {
            value: "```typescript\nlet x = 1;\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let doc_styles = DocumentStyles::default();
        let run = |width: Length, header: CodeBlockHeader, padding: CodeBlockPadding| {
            flatten_blocks(
                &doc,
                60,
                DocumentFlattenCtx {
                    wrap: true,
                    table_wrap: false,
                    table_width_mode: DocumentTableWidthMode::default(),
                    table_outer_frame: true,
                    table_column_separators: true,
                    table_row_separators: TableRowSeparators::default(),
                    table_cell_padding: 0,
                    table_border_variant: BorderStyle::Plain,
                    code_block_width: width,
                    code_block_padding: padding,
                    code_block_header: header,
                    code_block_inset: 1,
                    doc_styles: &doc_styles,
                    #[cfg(feature = "syntax-syntect")]
                    code_strategy: None,
                },
            )
        };

        // Flex(1) fills available width (60).
        let (flex_lines, _) = run(
            Length::Flex(1),
            CodeBlockHeader::None,
            CodeBlockPadding::None,
        );
        let flex_rect = flex_lines.iter().find_map(|l| match l.kind {
            VisualLineKind::CodeLine { rect_width, .. } => Some(rect_width),
            _ => None,
        });
        assert_eq!(flex_rect, Some(60));

        // Auto hugs widest line ("let x = 1;" = 10) + 1 left + 1 right = 12.
        let (auto_lines, _) = run(Length::Auto, CodeBlockHeader::None, CodeBlockPadding::None);
        let auto_rect = auto_lines.iter().find_map(|l| match l.kind {
            VisualLineKind::CodeLine { rect_width, .. } => Some(rect_width),
            _ => None,
        });
        assert_eq!(auto_rect, Some(12));

        // Auto expands to enclose a wider header (" typescript" = 1 + 1 + 10 = 12 -> 14 with insets).
        let (auto_hdr_lines, _) = run(
            Length::Auto,
            CodeBlockHeader::IconAndLabel,
            CodeBlockPadding::HalfBlock,
        );
        let hdr_rect = auto_hdr_lines.iter().find_map(|l| match &l.kind {
            VisualLineKind::CodeBlockChrome {
                kind: CodeBlockChromeKind::Header { .. },
                rect_width,
                ..
            } => Some(*rect_width),
            _ => None,
        });
        assert_eq!(hdr_rect, Some(14));

        // Px(30) and Percent(50) of 60 both produce rect_width 30.
        let (px_lines, _) = run(
            Length::Px(30),
            CodeBlockHeader::None,
            CodeBlockPadding::None,
        );
        let px_rect = px_lines.iter().find_map(|l| match l.kind {
            VisualLineKind::CodeLine { rect_width, .. } => Some(rect_width),
            _ => None,
        });
        assert_eq!(px_rect, Some(30));

        let (pct_lines, _) = run(
            Length::Percent(50),
            CodeBlockHeader::None,
            CodeBlockPadding::None,
        );
        let pct_rect = pct_lines.iter().find_map(|l| match l.kind {
            VisualLineKind::CodeLine { rect_width, .. } => Some(rect_width),
            _ => None,
        });
        assert_eq!(pct_rect, Some(30));

        // A header wider than a fixed rectangle is truncated to its content budget.
        let (narrow_hdr_lines, _) = run(
            Length::Px(8),
            CodeBlockHeader::IconAndLabel,
            CodeBlockPadding::Full,
        );
        for line in &narrow_hdr_lines {
            if let VisualLineKind::CodeBlockChrome {
                kind: CodeBlockChromeKind::Header { spans, .. },
                rect_width,
                ..
            } = &line.kind
            {
                assert_eq!(*rect_width, 8);
                assert_eq!(super::span_width(spans), 6);
            }
        }

        // Rectangles narrower than both insets keep their width and one content
        // column, and never emit an empty leading row.
        for px in 0..=3 {
            let (tiny_lines, _) = run(
                Length::Px(px),
                CodeBlockHeader::None,
                CodeBlockPadding::None,
            );
            let rows: Vec<_> = tiny_lines
                .iter()
                .filter_map(|l| match &l.kind {
                    VisualLineKind::CodeLine {
                        spans, rect_width, ..
                    } => Some((super::span_width(spans), *rect_width)),
                    _ => None,
                })
                .collect();
            assert_eq!(rows.len(), 10, "Px({px})");
            assert!(
                rows.iter().all(|&(w, rect)| w == 1 && rect == px.max(1)),
                "Px({px})"
            );
        }
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn code_block_inset_is_honored_and_shrinks_in_narrow_viewports() {
        use super::{DocumentFlattenCtx, VisualLineKind, flatten_blocks};
        use crate::style::{BorderStyle, Length};
        use crate::widgets::document_view::{
            CodeBlockHeader, CodeBlockPadding, ContentFormatter, DocumentStyles,
            DocumentTableWidthMode, FormatInput, MarkdownFormatter, TableRowSeparators,
        };

        let doc_styles = DocumentStyles::default();
        for (value, glyph_w) in [
            ("```\nabcdefghij\n```", 1),
            ("- item\n\n  ```\n  abcdefghij\n  ```", 1),
            ("```\n中文字符号\n```", 2),
        ] {
            let doc = MarkdownFormatter::default().format(FormatInput {
                value,
                content_type: Some("markdown"),
                document_styles: None,
            });
            let widths = [
                Length::Flex(1),
                Length::Auto,
                Length::Px(10),
                Length::Px(2),
                Length::Percent(60),
            ];
            for (width, inset, inner_w) in widths
                .into_iter()
                .flat_map(|w| (0..=3u16).map(move |i| (w, i)))
                .flat_map(|(w, i)| (1..=10u16).map(move |n| (w, i, n)))
            {
                let (lines, _) = flatten_blocks(
                    &doc,
                    inner_w,
                    DocumentFlattenCtx {
                        wrap: true,
                        table_wrap: false,
                        table_width_mode: DocumentTableWidthMode::default(),
                        table_outer_frame: true,
                        table_column_separators: true,
                        table_row_separators: TableRowSeparators::default(),
                        table_cell_padding: 0,
                        table_border_variant: BorderStyle::Plain,
                        code_block_width: width,
                        code_block_padding: CodeBlockPadding::Full,
                        code_block_header: CodeBlockHeader::Label,
                        code_block_inset: inset,
                        doc_styles: &doc_styles,
                        #[cfg(feature = "syntax-syntect")]
                        code_strategy: None,
                    },
                );
                for line in &lines {
                    let VisualLineKind::CodeLine {
                        spans,
                        indent_cols,
                        left_inset,
                        rect_width,
                        ..
                    } = &line.kind
                    else {
                        continue;
                    };
                    let avail = inner_w.saturating_sub(*indent_cols);
                    if avail < glyph_w {
                        continue; // Not even one glyph fits the viewport.
                    }
                    let used = left_inset + super::span_width(spans);
                    let ctx = format!("{value:?} {width:?} inset={inset} inner_w={inner_w}");
                    assert!(super::span_width(spans) >= 1, "{ctx}");
                    assert!(used <= *rect_width && *rect_width <= avail, "{ctx}");
                    // Explicit widths are not widened by the inset.
                    if let Length::Px(px) = width {
                        assert_eq!(*rect_width, px.max(glyph_w).min(avail), "{ctx}");
                    }
                    // With room for both insets and a glyph, the inset is
                    // honored on both sides.
                    if *rect_width >= 2 * inset + glyph_w {
                        assert_eq!(*left_inset, inset, "{ctx}");
                        assert!(used + inset <= *rect_width, "{ctx}");
                    }
                }
            }
        }
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn code_block_rect_grows_to_hold_wide_glyph_in_one_column_budget() {
        use super::{DocumentFlattenCtx, VisualLineKind, flatten_blocks};
        use crate::style::{BorderStyle, Length};
        use crate::widgets::document_view::{
            CodeBlockHeader, CodeBlockPadding, ContentFormatter, DocumentStyles,
            DocumentTableWidthMode, FormatInput, MarkdownFormatter, TableRowSeparators,
        };

        let doc = MarkdownFormatter::default().format(FormatInput {
            value: "```\n中a\n```",
            content_type: Some("markdown"),
            document_styles: None,
        });
        let doc_styles = DocumentStyles::default();
        let (lines, _) = flatten_blocks(
            &doc,
            60,
            DocumentFlattenCtx {
                wrap: true,
                table_wrap: false,
                table_width_mode: DocumentTableWidthMode::default(),
                table_outer_frame: true,
                table_column_separators: true,
                table_row_separators: TableRowSeparators::default(),
                table_cell_padding: 0,
                table_border_variant: BorderStyle::Plain,
                code_block_width: Length::Px(1),
                code_block_padding: CodeBlockPadding::None,
                code_block_header: CodeBlockHeader::None,
                code_block_inset: 1,
                doc_styles: &doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );
        for line in &lines {
            if let VisualLineKind::CodeLine {
                spans, rect_width, ..
            } = &line.kind
            {
                assert!(super::span_width(spans) <= *rect_width, "{line:?}");
            }
        }
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn code_block_padding_and_header_emit_expected_chrome_rows_and_preserve_flat_text() {
        use super::{
            CodeBlockChromeKind, DocumentFlattenCtx, VisualLineKind, build_visual_text_index,
            flatten_blocks, spans_plain_text,
        };
        use crate::style::{BorderStyle, Length};
        use crate::widgets::document_view::{
            CodeBlockHeader, CodeBlockPadding, ContentFormatter, DocumentStyles,
            DocumentTableWidthMode, FormatInput, MarkdownFormatter, TableRowSeparators,
        };

        let md = "Intro\n\n```rust\nlet a = 1;\n```\n\n```customlang\nx\n```\n\n```\nbare\n```";
        let doc = MarkdownFormatter::default().format(FormatInput {
            value: md,
            content_type: Some("markdown"),
            document_styles: None,
        });
        let doc_styles = DocumentStyles::default();
        let make_ctx = |padding: CodeBlockPadding, header: CodeBlockHeader| DocumentFlattenCtx {
            wrap: true,
            table_wrap: false,
            table_width_mode: DocumentTableWidthMode::default(),
            table_outer_frame: true,
            table_column_separators: true,
            table_row_separators: TableRowSeparators::default(),
            table_cell_padding: 0,
            table_border_variant: BorderStyle::Plain,
            code_block_width: Length::Flex(1),
            code_block_padding: padding,
            code_block_header: header,
            code_block_inset: 1,
            doc_styles: &doc_styles,
            #[cfg(feature = "syntax-syntect")]
            code_strategy: None,
        };

        let (plain_lines, _) = flatten_blocks(
            &doc,
            60,
            make_ctx(CodeBlockPadding::None, CodeBlockHeader::None),
        );
        let (decorated_lines, _) = flatten_blocks(
            &doc,
            60,
            make_ctx(CodeBlockPadding::HalfBlock, CodeBlockHeader::IconAndLabel),
        );

        let (_, _, _, plain_flat) = build_visual_text_index(&plain_lines, 60);
        let (_, _, _, decorated_flat) = build_visual_text_index(&decorated_lines, 60);
        assert_eq!(
            decorated_flat, plain_flat,
            "code-block chrome rows must not alter selectable flat_text"
        );

        let headers: Vec<String> = decorated_lines
            .iter()
            .filter_map(|l| match &l.kind {
                VisualLineKind::CodeBlockChrome {
                    kind: CodeBlockChromeKind::Header { spans, .. },
                    ..
                } => Some(spans_plain_text(spans)),
                _ => None,
            })
            .collect();
        // Known language gets icon + space + label; unknown language falls back to plain label;
        // untagged fence emits no header.
        assert_eq!(headers.len(), 2);
        assert!(headers[0].ends_with(" rust"), "got {:?}", headers[0]);
        assert!(headers[0].chars().count() > " rust".chars().count());
        assert_eq!(headers[1], "customlang");

        let (icon_lines, _) = flatten_blocks(
            &doc,
            60,
            make_ctx(CodeBlockPadding::HalfBlock, CodeBlockHeader::Icon),
        );
        let icon_headers: Vec<String> = icon_lines
            .iter()
            .filter_map(|l| match &l.kind {
                VisualLineKind::CodeBlockChrome {
                    kind: CodeBlockChromeKind::Header { spans, .. },
                    ..
                } => Some(spans_plain_text(spans)),
                _ => None,
            })
            .collect();
        // Icon alone for a known language; the label stands in when there is no icon.
        assert_eq!(icon_headers.len(), 2);
        assert_eq!(
            icon_headers[0].trim().chars().count(),
            1,
            "got {:?}",
            icon_headers[0]
        );
        assert!(!icon_headers[0].contains("rust"));
        assert_eq!(icon_headers[1], "customlang");

        let top_count = decorated_lines
            .iter()
            .filter(|l| {
                matches!(
                    l.kind,
                    VisualLineKind::CodeBlockChrome {
                        kind: CodeBlockChromeKind::TopHalfBlock,
                        ..
                    }
                )
            })
            .count();
        let bottom_count = decorated_lines
            .iter()
            .filter(|l| {
                matches!(
                    l.kind,
                    VisualLineKind::CodeBlockChrome {
                        kind: CodeBlockChromeKind::BottomHalfBlock,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(top_count, 3);
        assert_eq!(bottom_count, 3);
    }
}
