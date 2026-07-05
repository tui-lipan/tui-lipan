//! Layout measurement for [`DocumentView`](super::DocumentView).

use std::any::TypeId;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHasher};

use super::DocumentView;
use super::format::{ContentFormatter, FormatInput, PlainFormatter};
use super::planner::{auto_height_for_visual_plan, build_document_visual_metrics};
use crate::style::{Padding, ScrollbarVariant};

thread_local! {
    static MEASURE_FORMAT_CACHE: RefCell<FxHashMap<u64, Rc<super::format::FormattedDocument>>> =
        RefCell::new(FxHashMap::default());
}

const MEASURE_FORMAT_CACHE_MAX_ENTRIES: usize = 128;

/// Cross-instance cache for the expensive wrap-aware measure path.
///
/// `view()` rebuilds fresh [`DocumentView`] values each frame; the per-widget
/// `measure_cache` is always cold. Keys use [`document_measure_cache_key`] so
/// theme-only palette updates (not represented in that key) still hit here.
type DocumentGeometrySharedKey = (u64, u16);
type DocumentGeometrySharedCache = crate::utils::gen_cache::GenerationalCache<
    DocumentGeometrySharedKey,
    (u16, u16),
    rustc_hash::FxBuildHasher,
>;

thread_local! {
    static DOCUMENT_GEOMETRY_SHARED_CACHE: RefCell<DocumentGeometrySharedCache> =
        RefCell::new(DocumentGeometrySharedCache::new(DOCUMENT_GEOMETRY_SHARED_CACHE_CAP));
}

/// Per-generation cap. Large enough for wide scroll timelines × scrollbar width
/// probes without constant eviction; the generational cache (see
/// [`crate::utils::gen_cache`]) keeps a second generation so resize sweeps keep
/// hitting instead of thrashing.
const DOCUMENT_GEOMETRY_SHARED_CACHE_CAP: usize = 16_384;

fn document_geometry_shared_lookup(key: u64, mw: u16) -> Option<(u16, u16)> {
    DOCUMENT_GEOMETRY_SHARED_CACHE.with(|c| c.borrow().get(&(key, mw)).copied())
}

fn document_geometry_shared_store(key: u64, mw: u16, size: (u16, u16)) {
    DOCUMENT_GEOMETRY_SHARED_CACHE.with(|c| c.borrow_mut().insert((key, mw), size));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DocumentMeasureCacheEntry {
    pub key: u64,
    pub max_w: Option<u16>,
    pub size: (u16, u16),
}

pub(crate) fn gutter_total_width(base_width: u16, gap: u16) -> u16 {
    base_width.saturating_add(if base_width > 0 { gap } else { 0 })
}

pub(crate) fn resolved_gutter_base_width(
    total_lines: usize,
    line_numbers: bool,
    min_line_number_width: u8,
    line_number_separator: bool,
    line_number_content_gap: u16,
    gutter_col_width: u16,
) -> u16 {
    if gutter_col_width > 0 {
        gutter_col_width
    } else if line_numbers {
        gutter_width(
            total_lines,
            min_line_number_width,
            line_number_separator,
            line_number_content_gap,
        )
    } else {
        0
    }
}

pub(crate) fn standalone_scrollbar_cols(
    enabled: bool,
    variant: ScrollbarVariant,
    gap: u16,
    border: bool,
) -> u16 {
    let integrated_over_border =
        enabled && matches!(variant, ScrollbarVariant::Integrated) && border;
    if enabled && !integrated_over_border {
        1u16.saturating_add(gap)
    } else {
        0
    }
}

pub(crate) fn content_width_from_inner(
    inner_w: u16,
    gutter_total: u16,
    scrollbar_cols: u16,
) -> u16 {
    inner_w
        .saturating_sub(gutter_total)
        .saturating_sub(scrollbar_cols)
}

pub(crate) fn h_scrollbar_over_border(
    h_scrollbar: bool,
    h_scrollbar_variant: ScrollbarVariant,
    border: bool,
) -> bool {
    h_scrollbar && matches!(h_scrollbar_variant, ScrollbarVariant::Integrated) && border
}

pub(crate) fn h_scrollbar_visible(
    h_scrollbar: bool,
    wrap: bool,
    max_line_width: usize,
    content_w: u16,
) -> bool {
    h_scrollbar && !wrap && max_line_width > content_w as usize
}

pub(crate) fn visual_height_with_chrome(
    total_visual_lines: usize,
    padding: Padding,
    border: bool,
    h_scrollbar_visible: bool,
    h_scrollbar_over_border: bool,
) -> u16 {
    let mut visual_h = total_visual_lines.min(u16::MAX as usize) as u16;
    if h_scrollbar_visible && !h_scrollbar_over_border {
        visual_h = visual_h.saturating_add(1);
    }
    visual_h = visual_h.saturating_add(padding.vertical());
    if border {
        visual_h = visual_h.saturating_add(2);
    }
    visual_h
}

/// Measure the natural (unconstrained) size of a `DocumentView`.
pub fn measure_document_view(dv: &DocumentView) -> (u16, u16) {
    let line_count = dv.value.split('\n').count().max(1);
    let max_line_w = dv
        .value
        .split('\n')
        .map(|l| unicode_width::UnicodeWidthStr::width(l) as u16)
        .max()
        .unwrap_or(0);
    let gutter = resolved_gutter_base_width(
        line_count,
        dv.line_numbers,
        dv.min_line_number_width,
        dv.line_number_separator,
        dv.line_number_content_gap,
        dv.gutter_col_width,
    );
    let w = max_line_w.saturating_add(gutter_total_width(gutter, dv.gutter_gap));
    let h = line_count as u16;
    (w, h)
}

/// Measure with a width constraint, computing wrap-aware visual-line height.
///
/// When `height = Length::Auto`, this calls [`flatten_blocks`] with the actual
/// content width so the VStack measure pass allocates the correct number of
/// rows instead of the raw source-line count. This also accounts for a visible
/// standalone horizontal scrollbar when wrapping is disabled.
pub fn measure_document_view_constrained(dv: &DocumentView, max_w: Option<u16>) -> (u16, u16) {
    let cache_key = document_measure_cache_key(dv);
    for entry in dv.measure_cache.borrow().iter().flatten() {
        if entry.key == cache_key && entry.max_w == max_w {
            return entry.size;
        }
    }

    let is_auto_height = matches!(dv.resolved_height(), crate::style::Length::Auto);

    // Shared geometry cache: keyed by (cache_key, mw).  For the unconstrained
    // (`max_w = None`) and zero-width paths we use `mw = 0` as a sentinel -
    // both produce the natural size without any wrapping.
    if is_auto_height {
        let shared_mw = max_w.unwrap_or(0);
        if let Some(size) = document_geometry_shared_lookup(cache_key, shared_mw) {
            cache_measured_size(dv, cache_key, max_w, size);
            return size;
        }
    }

    let (nat_w, nat_h) = measure_document_view(dv);

    if !is_auto_height {
        let size = (nat_w, nat_h);
        cache_measured_size(dv, cache_key, max_w, size);
        return size;
    }

    let Some(mw) = max_w else {
        let size = (nat_w, nat_h);
        document_geometry_shared_store(cache_key, 0, size);
        cache_measured_size(dv, cache_key, max_w, size);
        return size;
    };

    if mw == 0 {
        let size = (nat_w, nat_h);
        document_geometry_shared_store(cache_key, 0, size);
        cache_measured_size(dv, cache_key, max_w, size);
        return size;
    }

    // Determine the effective outer width (mirrors reconcile logic).
    let layout_w = dv.width.resolve(mw, mw).min(mw);

    let border_w: u16 = if dv.border { 2 } else { 0 };
    let inner_w = layout_w
        .saturating_sub(border_w)
        .saturating_sub(dv.padding.horizontal());
    let scrollbar_cols = standalone_scrollbar_cols(
        dv.scrollbar,
        dv.scrollbar_config.variant,
        dv.scrollbar_config.gap,
        dv.border,
    );

    let mut dv_node = super::node::DocumentViewNode::from(dv.clone());

    // Keep measure path fully deterministic: no visual cache shortcuts while
    // we are converging reconcile/measure geometry behavior.
    dv_node.visual_cache.key = None;

    let formatter: &dyn ContentFormatter = dv
        .formatter
        .as_deref()
        .unwrap_or(&PlainFormatter as &dyn ContentFormatter);

    // Cache a measurement-only formatted document across width changes and
    // theme/style changes. Geometry does not depend on themed document styles,
    // so measurement should use the formatter's geometry-only cache key.
    // The formatted document is reused as `Rc` across both the per-widget and the
    // thread-local cache, so handing it to the metrics path is a refcount bump
    // rather than a deep clone of every block/line/span. Cloning it here was the
    // dominant measure-path cost while resizing diff-heavy sessions.
    let format_key = document_measure_format_cache_key(dv);
    let doc: Rc<super::format::FormattedDocument> = {
        let cached = dv.measure_format_cache.borrow();
        cached
            .as_ref()
            .filter(|(k, _)| *k == format_key)
            .map(|(_, doc)| Rc::clone(doc))
    }
    .unwrap_or_else(|| {
        let doc = MEASURE_FORMAT_CACHE.with(|cache| {
            if let Some(doc) = cache.borrow().get(&format_key) {
                return Rc::clone(doc);
            }

            let doc = Rc::new(formatter.measure_format(FormatInput {
                value: &dv.value,
                content_type: dv.content_type.as_deref(),
                document_styles: None,
            }));
            let mut cache = cache.borrow_mut();
            if cache.len() >= MEASURE_FORMAT_CACHE_MAX_ENTRIES
                && let Some(k) = cache.keys().next().copied()
            {
                cache.remove(&k);
            }
            cache.insert(format_key, Rc::clone(&doc));
            doc
        });
        *dv.measure_format_cache.borrow_mut() = Some((format_key, Rc::clone(&doc)));
        doc
    });

    let (content_w, visual_line_count, max_line_width) =
        build_document_visual_metrics(&dv_node, &doc, inner_w, scrollbar_cols);

    if content_w == 0 {
        let size = (layout_w, nat_h);
        document_geometry_shared_store(cache_key, mw, size);
        cache_measured_size(dv, cache_key, max_w, size);
        return size;
    }

    let visual_h =
        auto_height_for_visual_plan(&dv_node, visual_line_count, max_line_width, content_w);

    let size = (layout_w, visual_h);
    document_geometry_shared_store(cache_key, mw, size);
    cache_measured_size(dv, cache_key, max_w, size);
    size
}

fn cache_measured_size(dv: &DocumentView, key: u64, max_w: Option<u16>, size: (u16, u16)) {
    let new_entry = Some(DocumentMeasureCacheEntry { key, max_w, size });
    let mut slots = dv.measure_cache.borrow_mut();
    if slots[0] == new_entry {
        return;
    }
    slots[1] = slots[0];
    slots[0] = new_entry;
}

/// Hash that covers only the inputs to geometry-only formatting during
/// measurement. Width is excluded because the formatted document is
/// width-independent, and themed document styles are excluded because they do
/// not affect measured geometry.
fn document_measure_format_cache_key(dv: &DocumentView) -> u64 {
    let mut h = FxHasher::default();
    dv.layout_content_fingerprint().hash(&mut h);
    dv.content_type.hash(&mut h);
    dv.formatter
        .as_ref()
        .map(|f| f.as_any().type_id())
        .unwrap_or(TypeId::of::<PlainFormatter>())
        .hash(&mut h);
    dv.formatter
        .as_ref()
        .map(|f| f.measure_cache_key())
        .hash(&mut h);
    h.finish()
}

/// Base portion of the document measure cache key - all geometry fields except
/// the mutable split-wrap sync state.  Cached on [`DocumentView::measure_base_key_cache`]
/// to avoid re-hashing ~22 fields on every measurement call.
///
/// The cache is validated by [`DocumentView::layout_content_fingerprint`] which
/// itself guards against stale `Arc<str>` pointer reuse. This means a cloned-
/// then-mutated `DocumentView` still produces the correct key.
fn document_measure_base_key(dv: &DocumentView) -> u64 {
    let content_fp = dv.layout_content_fingerprint();
    if let Some((cached, guard_fp)) = dv.measure_base_key_cache.get()
        && guard_fp == content_fp
    {
        return cached;
    }

    let mut h = FxHasher::default();

    dv.layout_content_fingerprint().hash(&mut h);
    dv.content_type.hash(&mut h);
    dv.formatter
        .as_ref()
        .map(|f| f.measure_cache_key())
        .hash(&mut h);

    dv.width.hash(&mut h);
    dv.resolved_height().hash(&mut h);
    dv.wrap.hash(&mut h);
    dv.border.hash(&mut h);
    dv.padding.hash(&mut h);

    dv.scrollbar.hash(&mut h);
    dv.scrollbar_config.variant.hash(&mut h);
    dv.scrollbar_config.gap.hash(&mut h);
    dv.h_scrollbar.hash(&mut h);
    dv.focusable.hash(&mut h);
    dv.h_scrollbar_variant.hash(&mut h);

    dv.line_numbers.hash(&mut h);
    dv.min_line_number_width.hash(&mut h);
    dv.line_number_separator.hash(&mut h);
    dv.line_number_content_gap.hash(&mut h);
    dv.gutter_col_width.hash(&mut h);
    dv.gutter_gap.hash(&mut h);
    dv.peer_source_content_fingerprint().hash(&mut h);

    dv.table_wrap.hash(&mut h);
    dv.table_width_mode.hash(&mut h);
    dv.table_outer_frame.hash(&mut h);
    dv.table_column_separators.hash(&mut h);
    dv.table_row_separators.hash(&mut h);
    dv.table_cell_padding.hash(&mut h);
    dv.table_border_variant.hash(&mut h);

    let key = h.finish();
    dv.measure_base_key_cache.set(Some((key, content_fp)));
    key
}

fn document_measure_cache_key(dv: &DocumentView) -> u64 {
    let base = document_measure_base_key(dv);

    #[cfg(feature = "diff-view")]
    if dv.split_wrap_sync.is_some() {
        let mut h = FxHasher::default();
        base.hash(&mut h);
        let pane_width_hint =
            if let (Some(sync), Some(side)) = (&dv.split_wrap_sync, dv.split_wrap_side) {
                crate::widgets::diff_view::split_wrap_pane_widths(sync, side)
            } else {
                None
            };
        pane_width_hint.hash(&mut h);
        let split_wrap_sb = dv
            .split_wrap_sync
            .as_ref()
            .map(crate::widgets::diff_view::split_wrap_scrollbar_cols_pair);
        split_wrap_sb.hash(&mut h);
        let split_wrap_lp = dv
            .split_wrap_sync
            .as_ref()
            .map(crate::widgets::diff_view::split_wrap_layout_pass)
            .unwrap_or(0);
        split_wrap_lp.hash(&mut h);
        return h.finish();
    }

    base
}

/// Calculate the gutter width for line numbers.
pub(crate) fn gutter_width(
    total_lines: usize,
    min_digits: u8,
    show_separator: bool,
    content_gap: u16,
) -> u16 {
    // "NNN │ " when separator is enabled; digits only when disabled.
    let digits = if total_lines == 0 {
        1
    } else {
        (total_lines as f64).log10().floor() as u16 + 1
    }
    .max(min_digits as u16);
    digits
        .saturating_add(if show_separator { 2 } else { 0 })
        .saturating_add(content_gap)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    use super::*;
    use crate::style::{Color, Length, Style};
    #[cfg(feature = "markdown")]
    use crate::widgets::document_view::node::{DocumentFlattenCtx, flatten_blocks};
    use crate::widgets::{DocumentView, FormattedBlock, FormattedDocument, FormattedLine};

    #[test]
    fn constrained_measure_resolves_percent_width_from_max_width() {
        let dv = DocumentView::new("a short line")
            .width(Length::Percent(50))
            .height(Length::Auto)
            .wrap(true)
            .border(false);

        let (w, _) = measure_document_view_constrained(&dv, Some(60));
        assert_eq!(w, 30);
    }

    #[test]
    fn constrained_measure_percent_width_without_max_uses_natural_size() {
        let dv = DocumentView::new("hello")
            .width(Length::Percent(50))
            .height(Length::Auto)
            .wrap(true)
            .border(false);

        let natural = measure_document_view(&dv);
        let measured = measure_document_view_constrained(&dv, None);
        assert_eq!(measured, natural);
    }

    #[test]
    fn gutter_width_respects_min_digits() {
        assert_eq!(gutter_width(9, 0, true, 0), 3);
        assert_eq!(gutter_width(9, 4, true, 0), 6);
    }

    #[test]
    fn gutter_width_excludes_separator_when_disabled() {
        assert_eq!(gutter_width(9, 0, false, 0), 1);
        assert_eq!(gutter_width(120, 0, false, 0), 3);
    }

    #[test]
    fn gutter_width_includes_content_gap_and_separator_interaction() {
        assert_eq!(gutter_width(9, 0, false, 2), 3);
        assert_eq!(gutter_width(9, 0, true, 2), 5);
        assert_eq!(gutter_width(120, 0, true, 3), 8);
    }

    #[test]
    fn measure_respects_min_line_number_width() {
        let dv = DocumentView::new("x")
            .line_numbers(true)
            .min_line_number_width(4);
        let (w, h) = measure_document_view(&dv);
        assert_eq!(w, 7);
        assert_eq!(h, 1);
    }

    #[test]
    fn measure_line_numbers_shrinks_when_separator_disabled() {
        let with_separator = DocumentView::new("x")
            .line_numbers(true)
            .line_number_separator(true)
            .border(false);
        let without_separator = DocumentView::new("x")
            .line_numbers(true)
            .line_number_separator(false)
            .border(false);

        let (with_w, with_h) = measure_document_view(&with_separator);
        let (without_w, without_h) = measure_document_view(&without_separator);

        assert_eq!(with_h, without_h);
        assert_eq!(with_w, without_w.saturating_add(2));
    }

    #[test]
    fn measure_line_numbers_includes_content_gap() {
        let dv = DocumentView::new("x")
            .line_numbers(true)
            .line_number_separator(false)
            .line_number_content_gap(2)
            .border(false);
        let (w, h) = measure_document_view(&dv);
        assert_eq!(w, 4);
        assert_eq!(h, 1);
    }

    #[test]
    fn auto_height_counts_standalone_horizontal_scrollbar_row() {
        let dv = DocumentView::new("123456789\nabc")
            .width(Length::Px(5))
            .height(Length::Auto)
            .wrap(false)
            .scrollbar(false)
            .h_scrollbar(true)
            .border(false);

        let (w, h) = measure_document_view_constrained(&dv, Some(5));
        assert_eq!(w, 5);
        assert_eq!(h, 3);
    }

    #[test]
    fn auto_height_with_custom_gutter_uses_gutter_width_for_overflow() {
        let dv = DocumentView::new("123456789\nabc")
            .width(Length::Px(10))
            .height(Length::Auto)
            .line_numbers(false)
            .gutter_lines(Arc::new(vec![vec![], vec![]]), 5)
            .wrap(false)
            .scrollbar(false)
            .h_scrollbar(true)
            .border(false);

        let (w, h) = measure_document_view_constrained(&dv, Some(10));
        assert_eq!(w, 10);
        assert_eq!(h, 3);
    }

    #[test]
    fn measure_includes_gutter_gap() {
        let dv = DocumentView::new("x")
            .line_numbers(true)
            .gutter_inset(1)
            .border(false);
        let (w, h) = measure_document_view(&dv);
        assert_eq!(w, 5);
        assert_eq!(h, 1);
    }

    #[test]
    fn constrained_measure_cache_reuses_and_invalidates_by_value() {
        let dv = DocumentView::new("hello world")
            .width(Length::Px(10))
            .height(Length::Auto)
            .wrap(true)
            .border(false);

        let first = measure_document_view_constrained(&dv, Some(10));
        let second = measure_document_view_constrained(&dv, Some(10));
        assert_eq!(first, second);

        let mut changed = dv.clone();
        changed.value = "hello world\nsecond line".into();
        let third = measure_document_view_constrained(&changed, Some(10));
        assert_ne!(first, third);
    }

    #[test]
    fn measure_format_cache_key_ignores_themed_document_styles() {
        let base = DocumentView::new("hello world\nsecond line")
            .width(Length::Px(20))
            .height(Length::Auto)
            .wrap(true)
            .border(false);

        let mut themed = base.clone();
        themed.doc_styles.heading_styles[0] = Style::new().fg(Color::rgb(10, 20, 30)).bold();
        themed.doc_styles.list_item_style = Style::new().fg(Color::rgb(200, 100, 50));

        assert_eq!(
            document_measure_format_cache_key(&base),
            document_measure_format_cache_key(&themed)
        );
        assert_eq!(
            measure_document_view_constrained(&base, Some(20)),
            measure_document_view_constrained(&themed, Some(20))
        );
    }

    #[derive(Clone)]
    struct CountingMeasureFormatter {
        calls: Rc<Cell<usize>>,
    }

    impl ContentFormatter for CountingMeasureFormatter {
        fn clone_box(&self) -> Box<dyn ContentFormatter> {
            Box::new(self.clone())
        }

        fn format(&self, input: FormatInput<'_>) -> FormattedDocument {
            self.measure_format(input)
        }

        fn measure_format(&self, input: FormatInput<'_>) -> FormattedDocument {
            self.calls.set(self.calls.get() + 1);
            FormattedDocument {
                blocks: vec![FormattedBlock::Lines(vec![FormattedLine {
                    spans: vec![crate::style::Span::new(input.value)],
                    source_line: 0,
                    indent: 0,
                    links: Vec::new(),
                }])],
            }
        }

        fn measure_cache_key(&self) -> u64 {
            7
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn global_measure_format_cache_reuses_across_fresh_document_views() {
        let calls = Rc::new(Cell::new(0));
        let formatter = CountingMeasureFormatter {
            calls: calls.clone(),
        };

        let a = DocumentView::new("shared content")
            .formatter(formatter.clone())
            .width(Length::Px(20))
            .height(Length::Auto)
            .wrap(true)
            .border(false);
        let b = DocumentView::new("shared content")
            .formatter(formatter)
            .width(Length::Px(20))
            .height(Length::Auto)
            .wrap(true)
            .border(false);

        let _ = measure_document_view_constrained(&a, Some(20));
        let _ = measure_document_view_constrained(&b, Some(20));

        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn inline_wrapped_default_flex_height_matches_explicit_auto_measurement() {
        let explicit_auto = DocumentView::new("hello _now_ms wrapped content")
            .width(Length::Px(10))
            .height(Length::Auto)
            .wrap(true)
            .scrollbar(false)
            .h_scrollbar(false)
            .focusable(false)
            .border(false);

        let implicit_auto = DocumentView::new("hello _now_ms wrapped content")
            .width(Length::Px(10))
            .wrap(true)
            .scrollbar(false)
            .h_scrollbar(false)
            .focusable(false)
            .border(false);

        assert_eq!(
            measure_document_view_constrained(&implicit_auto, Some(10)),
            measure_document_view_constrained(&explicit_auto, Some(10))
        );
    }

    #[test]
    fn focusable_wrapped_default_flex_does_not_implicitly_auto_height() {
        let dv = DocumentView::new("hello _now_ms wrapped content")
            .width(Length::Px(10))
            .wrap(true)
            .scrollbar(false)
            .h_scrollbar(false)
            .focusable(true)
            .border(false);

        let natural = measure_document_view(&dv);
        assert_eq!(measure_document_view_constrained(&dv, Some(10)), natural);
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn markdown_list_measure_matches_rendered_visual_line_count() {
        use crate::widgets::document_view::MarkdownFormatter;

        let dv = DocumentView::new(
            "1. I dug deeper and the remaining memory growth was not the keyed screen anymore - it was PTY transport/state being kept app-global and surviving session switches.",
        )
        .formatter(MarkdownFormatter::default())
        .content_type("markdown")
        .width(Length::Px(60))
        .height(Length::Auto)
        .wrap(true)
        .border(false)
        .scrollbar(false)
        .h_scrollbar(false);

        let measured = measure_document_view_constrained(&dv, Some(60));

        let formatter = dv.formatter.as_deref().expect("formatter should be set");
        let doc = formatter.format(FormatInput {
            value: &dv.value,
            content_type: dv.content_type.as_deref(),
            document_styles: Some(&dv.doc_styles),
        });
        let (visual_lines, _) = super::super::node::flatten_blocks(
            &doc,
            60,
            super::super::node::DocumentFlattenCtx {
                wrap: dv.wrap,
                table_wrap: dv.table_wrap,
                table_width_mode: dv.table_width_mode,
                table_outer_frame: dv.table_outer_frame,
                table_column_separators: dv.table_column_separators,
                table_row_separators: dv.table_row_separators,
                table_cell_padding: dv.table_cell_padding,
                table_border_variant: dv.table_border_variant,
                doc_styles: &dv.doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );

        assert_eq!(
            measured.1,
            visual_lines.len() as u16,
            "measure path should match render flattening for markdown lists"
        );
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn markdown_mixed_blocks_measure_matches_rendered_visual_line_count() {
        use crate::widgets::document_view::MarkdownFormatter;

        let dv = DocumentView::new(
            "This is a long introductory paragraph that should wrap when measured inside a narrow frame.\n\n> A quoted paragraph that also wraps and used to be easy to undercount when measurement drifted from reconcile.\n\n- First bullet with enough text to wrap across multiple rows.\n- Second bullet with another long sentence to keep the layout honest.",
        )
        .formatter(MarkdownFormatter::default())
        .content_type("markdown")
        .width(Length::Px(36))
        .height(Length::Auto)
        .wrap(true)
        .border(false)
        .scrollbar(false)
        .h_scrollbar(false);

        let measured = measure_document_view_constrained(&dv, Some(36));

        let formatter = dv.formatter.as_deref().expect("formatter should be set");
        let doc = formatter.format(FormatInput {
            value: &dv.value,
            content_type: dv.content_type.as_deref(),
            document_styles: Some(&dv.doc_styles),
        });
        let (visual_lines, _) = flatten_blocks(
            &doc,
            36,
            DocumentFlattenCtx {
                wrap: dv.wrap,
                table_wrap: dv.table_wrap,
                table_width_mode: dv.table_width_mode,
                table_outer_frame: dv.table_outer_frame,
                table_column_separators: dv.table_column_separators,
                table_row_separators: dv.table_row_separators,
                table_cell_padding: dv.table_cell_padding,
                table_border_variant: dv.table_border_variant,
                doc_styles: &dv.doc_styles,
                #[cfg(feature = "syntax-syntect")]
                code_strategy: None,
            },
        );

        assert_eq!(
            measured.1,
            visual_lines.len() as u16,
            "measure path should match render flattening for mixed markdown blocks"
        );
    }

    #[test]
    fn plain_lines_measure_matches_reconcile_across_widths() {
        // Exercises the Lines-only count fast path in `build_document_visual_metrics`
        // against the authoritative reconcile (which uses the full flatten). A
        // mismatch here means the count path drifted from `flatten_blocks`.
        let text = "a short line\n\
            this is a considerably longer line that will wrap several times at narrow widths\n\
            mixed 你好 width 世界 characters that also need to wrap correctly\n\
            \n\
            tab\tand  multiple   spaces   between   words";
        for width in 8..50u16 {
            let dv = DocumentView::new(text)
                .width(Length::Px(width))
                .height(Length::Auto)
                .wrap(true)
                .border(false)
                .scrollbar(false)
                .h_scrollbar(false);

            let (_, measured_h) = measure_document_view_constrained(&dv, Some(width));

            let root: crate::core::element::Element =
                crate::widgets::VStack::new().child(dv.clone()).into();
            let mut tree = crate::core::node::NodeTree::new();
            crate::layout::LayoutEngine::reconcile_with_focus(
                &mut tree,
                &root,
                crate::style::Rect {
                    x: 0,
                    y: 0,
                    w: width,
                    h: 400,
                },
                None,
            );
            let dv_id = tree.node(tree.root).children[0];
            let node = tree.node(dv_id);
            assert_eq!(
                measured_h, node.rect.h,
                "at width={width}: measured_h={measured_h} != reconciled_h={}",
                node.rect.h
            );
        }
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn markdown_code_block_measure_matches_reconcile_across_widths() {
        use crate::widgets::document_view::MarkdownFormatter;

        let md = "Hello\n\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n\nWorld";
        for width in 20..60u16 {
            let dv = DocumentView::new(md)
                .formatter(MarkdownFormatter::default())
                .content_type("markdown")
                .width(Length::Px(width))
                .height(Length::Auto)
                .wrap(true)
                .border(false)
                .scrollbar(false)
                .h_scrollbar(false);

            let (_, measured_h) = measure_document_view_constrained(&dv, Some(width));

            let root: crate::core::element::Element =
                crate::widgets::VStack::new().child(dv.clone()).into();
            let mut tree = crate::core::node::NodeTree::new();
            crate::layout::LayoutEngine::reconcile_with_focus(
                &mut tree,
                &root,
                crate::style::Rect {
                    x: 0,
                    y: 0,
                    w: width,
                    h: 200,
                },
                None,
            );
            let dv_id = tree.node(tree.root).children[0];
            let node = tree.node(dv_id);
            assert_eq!(
                measured_h, node.rect.h,
                "at width={width}: measured_h={measured_h} != reconciled_h={}",
                node.rect.h
            );
        }
    }

    #[cfg(feature = "markdown")]
    #[test]
    fn markdown_code_block_measure_includes_left_inset_width_for_hscroll() {
        use crate::widgets::document_view::MarkdownFormatter;
        use crate::widgets::document_view::node::CODE_BLOCK_LEFT_INSET_COLS;

        let dv = DocumentView::new("```rust\n123456789\n```")
            .formatter(MarkdownFormatter::default())
            .content_type("markdown")
            .width(Length::Px(9))
            .height(Length::Auto)
            .wrap(false)
            .scrollbar(false)
            .h_scrollbar(true)
            .border(false);

        let (_, h) = measure_document_view_constrained(&dv, Some(9));

        // One code line (9 chars) plus a 1-cell code inset forces h-scroll
        // at content width 9, so measured height includes scrollbar row.
        assert_eq!(CODE_BLOCK_LEFT_INSET_COLS, 1);
        assert_eq!(h, 2);
    }
}
