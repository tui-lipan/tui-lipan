//! Reconciliation for [`DocumentView`](super::DocumentView).

#[cfg(test)]
use std::cell::Cell;
use std::hash::{Hash, Hasher};
#[cfg(not(test))]
use std::sync::atomic::{AtomicU64, Ordering};

use rustc_hash::FxHasher;

#[cfg(not(test))]
static VISUAL_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
#[cfg(not(test))]
static VISUAL_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
#[cfg(not(test))]
static VISUAL_CACHE_INELIGIBLE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
thread_local! {
    static VISUAL_CACHE_HITS: Cell<u64> = const { Cell::new(0) };
    static VISUAL_CACHE_MISSES: Cell<u64> = const { Cell::new(0) };
    static VISUAL_CACHE_INELIGIBLE: Cell<u64> = const { Cell::new(0) };
}

#[cfg(not(test))]
fn visual_cache_hits() -> u64 {
    VISUAL_CACHE_HITS.load(Ordering::Relaxed)
}

#[cfg(test)]
fn visual_cache_hits() -> u64 {
    VISUAL_CACHE_HITS.with(Cell::get)
}

#[cfg(not(test))]
fn visual_cache_misses() -> u64 {
    VISUAL_CACHE_MISSES.load(Ordering::Relaxed)
}

#[cfg(test)]
fn visual_cache_misses() -> u64 {
    VISUAL_CACHE_MISSES.with(Cell::get)
}

#[cfg(not(test))]
fn visual_cache_ineligible() -> u64 {
    VISUAL_CACHE_INELIGIBLE.load(Ordering::Relaxed)
}

#[cfg(test)]
fn visual_cache_ineligible() -> u64 {
    VISUAL_CACHE_INELIGIBLE.with(Cell::get)
}

#[cfg(not(test))]
fn increment_visual_cache_counter(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

#[cfg(test)]
fn reset_visual_cache_counter(counter: &'static std::thread::LocalKey<Cell<u64>>) {
    counter.with(|cell| cell.set(0));
}

#[cfg(test)]
fn increment_visual_cache_counter(counter: &'static std::thread::LocalKey<Cell<u64>>) {
    counter.with(|cell| cell.set(cell.get().saturating_add(1)));
}

#[cfg(test)]
pub fn visual_cache_stats() -> (u64, u64, u64) {
    (
        visual_cache_hits(),
        visual_cache_misses(),
        visual_cache_ineligible(),
    )
}

#[cfg(test)]
pub fn reset_visual_cache_stats() {
    reset_visual_cache_counter(&VISUAL_CACHE_HITS);
    reset_visual_cache_counter(&VISUAL_CACHE_MISSES);
    reset_visual_cache_counter(&VISUAL_CACHE_INELIGIBLE);
}

fn maybe_log_cache_stats() {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    let enabled = *ENABLED.get_or_init(|| std::env::var_os("TUI_LIPAN_CACHE_STATS").is_some());
    if !enabled {
        return;
    }
    let total = visual_cache_hits() + visual_cache_misses() + visual_cache_ineligible();
    if total != 0 && total % 2000 == 0 {
        let h = visual_cache_hits();
        let m = visual_cache_misses();
        let i = visual_cache_ineligible();
        let attempted = h + m;
        let pct = if attempted > 0 {
            (h as f64 / attempted as f64) * 100.0
        } else {
            0.0
        };
        eprintln!("[tui-lipan visual_cache] hits={h} misses={m} ineligible={i} hit_rate={pct:.1}%");
    }
}

#[cfg(feature = "profiling-tracing")]
use tracing::trace_span;

use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::resolve_rect_with_auto;
use crate::style::LayoutConstraints;
#[cfg(feature = "diff-view")]
use crate::widgets::diff_view::{
    peer_pass1_source_heights, record_pass1_source_heights, split_wrap_layout_pass,
    split_wrap_pane_widths, split_wrap_scrollbar_cols_pair,
};

use crate::widgets::scroll::SmoothScrollState;

use super::DocumentView;
use super::format::PlainFormatter;
use super::layout::{
    h_scrollbar_visible, measure_document_view, measure_document_view_constrained,
    standalone_scrollbar_cols,
};
use super::node::DocumentViewNode;
#[cfg(feature = "diff-view")]
use super::node::source_visual_heights;
use super::planner::{
    apply_visual_plan_to_node, auto_height_for_visual_plan, build_document_visual_plan,
    should_use_visual_auto_height, viewport_height_for_visual_plan,
};

#[cfg(feature = "diff-view")]
fn split_wrap_visual_cache_hash(dv_node: &DocumentViewNode) -> Option<u64> {
    if dv_node.peer_source_lines.is_none() && dv_node.split_wrap_sync.is_none() {
        return Some(0);
    }

    let (Some(sync), Some(side)) = (&dv_node.split_wrap_sync, dv_node.split_wrap_side) else {
        return None;
    };
    let pass = split_wrap_layout_pass(sync);
    if pass == 0 {
        return None;
    }

    let mut h = FxHasher::default();
    pass.hash(&mut h);
    side.hash(&mut h);
    split_wrap_pane_widths(sync, side).hash(&mut h);
    split_wrap_scrollbar_cols_pair(sync).hash(&mut h);

    if pass == 2 {
        let peer_heights = peer_pass1_source_heights(sync, side)?;
        peer_heights.len().hash(&mut h);
        peer_heights.as_slice().hash(&mut h);
    }

    Some(h.finish())
}

#[cfg(feature = "diff-view")]
fn record_split_wrap_pass1_cache_hit(dv_node: &DocumentViewNode) {
    let (Some(sync), Some(side)) = (&dv_node.split_wrap_sync, dv_node.split_wrap_side) else {
        return;
    };
    if split_wrap_layout_pass(sync) != 1 {
        return;
    }

    let heights = source_visual_heights(&dv_node.visual_cache.lines);
    record_pass1_source_heights(sync, side, &heights);
}

/// Reconcile a `DocumentView` element into the node tree.
pub fn reconcile_document_view(
    tree: &mut NodeTree,
    id: NodeId,
    _parent: Option<NodeId>,
    dv: &DocumentView,
    rect: crate::style::Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    #[cfg(feature = "profiling-tracing")]
    let _span = trace_span!("document_view.reconcile").entered();

    let (nat_w, nat_h) = if matches!(dv.resolved_height(), crate::style::Length::Auto) {
        measure_document_view_constrained(dv, Some(rect.w))
    } else {
        measure_document_view(dv)
    };
    let available_h = rect.h;
    let mut rect = resolve_rect_with_auto(
        rect,
        constraints,
        dv.width,
        dv.resolved_height(),
        nat_w,
        nat_h,
    );

    // Content hash for cache invalidation.
    let value_hash = {
        let mut h = FxHasher::default();
        dv.value.hash(&mut h);
        h.finish()
    };

    // Extract old state from an existing node, or from a keyed ScrollView row
    // that was virtualized out of the node tree and is now visible again.
    let old_node_state = {
        let node = tree.node_mut(id);
        if let NodeKind::DocumentView(ref mut existing) = node.kind {
            Some((
                existing.scroll_offset,
                existing.scroll_override,
                existing.h_scroll_offset,
                existing.h_scroll_override,
                std::mem::take(&mut existing.smooth_scroll),
                existing.cancelled_scroll_to_source_line,
                std::mem::take(&mut existing.format_cache),
                std::mem::take(&mut existing.visual_cache),
                existing.selection_cursor,
                existing.selection_anchor,
                existing.table_rect_selection.take(),
                existing.content_hash,
            ))
        } else {
            None
        }
    };
    let (
        old_offset,
        old_scroll_override,
        old_h_offset,
        old_h_override,
        old_smooth_scroll,
        old_cancelled_scroll_to_source_line,
        old_format_cache,
        old_visual_cache,
        old_selection_cursor,
        old_selection_anchor,
        old_table_rect_selection,
        old_content_hash,
    ) = if let Some(state) = old_node_state {
        state
    } else if let Some(saved) = tree.take_next_offscreen_doc_restore() {
        let saved_had_selection =
            saved.selection_anchor.is_some() || saved.table_rect_selection.is_some();
        let (selection_cursor, selection_anchor, table_rect_selection) = if saved_had_selection {
            (
                saved.selection_cursor,
                saved.selection_anchor,
                saved.table_rect_selection,
            )
        } else {
            (0, None, None)
        };
        (
            0,
            None,
            0,
            None,
            SmoothScrollState::default(),
            None,
            saved.format_cache,
            saved.visual_cache,
            selection_cursor,
            selection_anchor,
            table_rect_selection,
            0,
        )
    } else {
        (
            0,
            None,
            0,
            None,
            SmoothScrollState::default(),
            None,
            super::format::FormatCache::default(),
            super::node::VisualCache::default(),
            0,
            None,
            None,
            0,
        )
    };

    let content_changed = old_content_hash != 0 && value_hash != old_content_hash;
    #[cfg(feature = "diff-view")]
    let scroll_anchor_source_line = if dv.diff_context_separator_click.is_some()
        && content_changed
        && dv.scroll_offset.is_none()
        && dv.scroll_to_source_line.is_none()
    {
        old_visual_cache.source_line_map.get(old_offset).copied()
    } else {
        None
    };
    #[cfg(not(feature = "diff-view"))]
    let scroll_anchor_source_line: Option<usize> = None;
    #[cfg(not(feature = "diff-view"))]
    let _ = content_changed;

    // Build the new node.
    let mut dv_node = DocumentViewNode::from(dv.clone());
    dv_node.content_hash = value_hash;
    dv_node.format_cache = old_format_cache;
    dv_node.visual_cache = old_visual_cache;
    dv_node.smooth_scroll = old_smooth_scroll;
    dv_node.selection_cursor = old_selection_cursor;
    dv_node.selection_anchor = old_selection_anchor;
    dv_node.table_rect_selection = old_table_rect_selection;

    // ── Format cache update ─────────────────────────────────────────────
    let formatter: &dyn super::ContentFormatter = dv_node
        .formatter
        .as_deref()
        .unwrap_or(&PlainFormatter as &dyn super::ContentFormatter);
    dv_node.format_cache.update(
        formatter,
        &dv_node.value,
        dv_node.content_type.as_deref(),
        &dv_node.doc_styles,
    );

    // ── Compute inner dimensions (accounting for border + padding) ──────
    let inner = rect.inner(dv_node.border, dv_node.padding);

    let scrollbar_cols = standalone_scrollbar_cols(
        dv_node.scrollbar,
        dv_node.scrollbar_variant,
        dv_node.scrollbar_gap,
        dv_node.border,
    );

    // ── Visual cache lookup ─────────────────────────────────────────────
    // Skip the (expensive) planner when the inputs that affect visual-line
    // output haven't changed. Keyed on content hash (`value_hash`, already
    // computed above) rather than Arc pointer identity, because callers
    // routinely rebuild Arc<str> content each frame from formatted strings
    // even when bytes are identical. Split-wrap DiffView panes include the
    // current dual-pass state in the key; Visual line-numbering still rebuilds
    // because it re-flattens once the gutter width is known.
    #[cfg(feature = "diff-view")]
    let split_wrap_hash = split_wrap_visual_cache_hash(&dv_node);
    #[cfg(not(feature = "diff-view"))]
    let split_wrap_hash = 0_u64;
    // Visual line-number mode re-flattens once the gutter width is known,
    // creating a chicken-and-egg with the cache key — but only when line
    // numbers are actually rendered. When they're off the mode is inert.
    let visual_line_number_active = dv_node.line_numbers
        && matches!(
            dv_node.line_number_mode,
            super::DocumentLineNumberMode::Visual
        );
    #[cfg(feature = "diff-view")]
    let split_wrap_cache_ready = split_wrap_hash.is_some();
    #[cfg(not(feature = "diff-view"))]
    let split_wrap_cache_ready = true;
    let cache_eligible = split_wrap_cache_ready && !visual_line_number_active;
    let visual_key = if cache_eligible {
        let source_line_count = dv_node.value.split('\n').count().max(1);
        let gutter = super::layout::resolved_gutter_base_width(
            source_line_count,
            dv_node.line_numbers,
            dv_node.min_line_number_width,
            dv_node.line_number_separator,
            dv_node.line_number_content_gap,
            dv_node.gutter_col_width,
        );
        let total_gutter = super::layout::gutter_total_width(gutter, dv_node.gutter_gap);
        let static_content_w =
            super::layout::content_width_from_inner(inner.w, total_gutter, scrollbar_cols);
        let value_id = value_hash;
        let formatter_hash = formatter.cache_key();
        #[cfg(feature = "syntax-syntect")]
        let syntax_strategy_hash = dv_node
            .code_syntax_strategy
            .as_deref()
            .map_or(0, crate::widgets::TextAreaColorStrategy::cache_key);
        #[cfg(not(feature = "syntax-syntect"))]
        let syntax_strategy_hash = 0_u64;
        let table_and_styles_hash = {
            let mut h = FxHasher::default();
            dv_node.table_wrap.hash(&mut h);
            dv_node.table_width_mode.hash(&mut h);
            dv_node.table_outer_frame.hash(&mut h);
            dv_node.table_column_separators.hash(&mut h);
            dv_node.table_row_separators.hash(&mut h);
            dv_node.table_cell_padding.hash(&mut h);
            dv_node.table_border_variant.hash(&mut h);
            dv_node.doc_styles.heading_styles.hash(&mut h);
            dv_node.doc_styles.code_inline_style.hash(&mut h);
            dv_node.doc_styles.code_block_style.hash(&mut h);
            dv_node.doc_styles.emphasis_style.hash(&mut h);
            dv_node.doc_styles.strong_style.hash(&mut h);
            dv_node.doc_styles.strikethrough_style.hash(&mut h);
            dv_node.doc_styles.link_style.hash(&mut h);
            dv_node.doc_styles.blockquote_bar_style.hash(&mut h);
            dv_node.doc_styles.table_border_style.hash(&mut h);
            dv_node.doc_styles.table_header_style.hash(&mut h);
            dv_node.doc_styles.hr_style.hash(&mut h);
            dv_node.doc_styles.list_item_style.hash(&mut h);
            dv_node.doc_styles.list_enumeration_style.hash(&mut h);
            dv_node.doc_styles.diagram_node_fill_style.hash(&mut h);
            dv_node.doc_styles.diagram_node_border_style.hash(&mut h);
            dv_node.doc_styles.diagram_node_label_style.hash(&mut h);
            dv_node.doc_styles.diagram_edge_style.hash(&mut h);
            dv_node.doc_styles.diagram_muted_style.hash(&mut h);
            h.finish()
        };
        #[cfg(feature = "diff-view")]
        let split_wrap_hash_value = split_wrap_hash.unwrap_or(0);
        #[cfg(not(feature = "diff-view"))]
        let split_wrap_hash_value = split_wrap_hash;
        Some((
            value_id,
            formatter_hash,
            syntax_strategy_hash,
            static_content_w,
            dv_node.wrap,
            table_and_styles_hash,
            split_wrap_hash_value,
        ))
    } else {
        dv_node.visual_cache.key = None;
        increment_visual_cache_counter(&VISUAL_CACHE_INELIGIBLE);
        maybe_log_cache_stats();
        None
    };

    // ── Compute visual lines ────────────────────────────────────────────
    let content_w = if let Some(key) = visual_key
        && dv_node.visual_cache.promote(&key)
    {
        // Cache hit: reuse cached lines, just re-export derived fields.
        increment_visual_cache_counter(&VISUAL_CACHE_HITS);
        maybe_log_cache_stats();
        dv_node.total_visual_lines = dv_node.visual_cache.lines.len();
        dv_node.max_line_width = dv_node.visual_cache.max_line_width;
        #[cfg(feature = "diff-view")]
        record_split_wrap_pass1_cache_hit(&dv_node);
        key.3
    } else {
        if let Some(key) = visual_key {
            increment_visual_cache_counter(&VISUAL_CACHE_MISSES);
            maybe_log_cache_stats();
            dv_node.visual_cache.preserve_active_for_insert(&key);
        }
        let plan = build_document_visual_plan(
            &dv_node,
            &dv_node.format_cache.document,
            inner.w,
            scrollbar_cols,
        );
        let content_w = plan.content_w;
        apply_visual_plan_to_node(&mut dv_node, plan);
        dv_node.visual_cache.key = visual_key;
        content_w
    };

    let h_scrollbar_visible = h_scrollbar_visible(
        dv_node.h_scrollbar,
        dv_node.wrap,
        dv_node.max_line_width as usize,
        content_w,
    );

    // ── Auto height correction ───────────────────────────────────────────
    // `measure_document_view` returns source-line count; for Length::Auto we
    // need the wrap-aware visual-line count now that rebuild_visual has run.
    if should_use_visual_auto_height(dv.resolved_height()) {
        let visual_h = auto_height_for_visual_plan(
            &dv_node,
            dv_node.total_visual_lines,
            dv_node.max_line_width,
            content_w,
        );
        // Use the planner's intrinsic height. Clipping with `min(available_h)` caused
        // `total_visual_lines > viewport_h` whenever stack measure underestimated the
        // slot (nested Auto + ScrollView): the DocumentView looked scrollable though
        // the outer ScrollView should own vertical scrolling. Stack measure now uses
        // the same `compute_stack_layout` engine as reconcile so `available_h` should
        // match; clamp_height still applies min_h / max_h from layout constraints.
        rect.h = visual_h;
        rect.h = constraints.clamp_height(rect.h, available_h);
    }

    let flat = &dv_node.visual_cache.flat_text;
    dv_node.selection_cursor = snap_to_char_boundary(flat, dv_node.selection_cursor);
    dv_node.selection_anchor = dv_node
        .selection_anchor
        .map(|a| snap_to_char_boundary(flat, a));
    if let Some(sel) = &dv_node.table_rect_selection {
        let valid = dv_node.visual_cache.lines.iter().any(|line| {
            if let super::node::VisualLineKind::TableRow {
                table_id,
                row_index,
                widths,
                ..
            } = &line.kind
            {
                *table_id == sel.table_id
                    && *row_index >= sel.row_start
                    && *row_index <= sel.row_end
                    && sel.col_end < widths.len()
            } else {
                false
            }
        });
        if !valid {
            dv_node.table_rect_selection = None;
        }
    }

    let viewport_h =
        viewport_height_for_visual_plan(&dv_node, rect, dv_node.max_line_width, content_w);

    // ── Vertical scroll offset ──────────────────────────────────────────
    let mut current_override = old_scroll_override;
    let controlled_offset_interrupts_smooth_target = dv.scroll_offset.is_some()
        && dv_node.smooth_scroll.is_animating()
        && dv_node.scroll_to_source_line.is_some();
    let controlled_offset_changed = if let Some(forced) = dv.scroll_offset
        && (Some(forced) != old_scroll_override || controlled_offset_interrupts_smooth_target)
    {
        current_override = Some(forced);
        true
    } else {
        false
    };

    let controlled_offset_suppresses_target =
        controlled_offset_changed && dv_node.scroll_to_source_line.is_some();
    let source_target_suppressed = controlled_offset_suppresses_target
        || (dv_node.scroll_to_source_line.is_some()
            && dv_node.scroll_to_source_line == old_cancelled_scroll_to_source_line);
    dv_node.cancelled_scroll_to_source_line = if source_target_suppressed {
        dv_node.scroll_to_source_line
    } else {
        None
    };

    let target_offset = if source_target_suppressed {
        None
    } else if let Some(target_source) = dv_node.scroll_to_source_line {
        // Binary search for the first visual line matching this source line.
        Some(
            dv_node
                .visual_cache
                .lines
                .iter()
                .position(|vl| vl.source_line >= target_source)
                .unwrap_or(0),
        )
    } else {
        None
    };

    let max_offset = dv_node
        .total_visual_lines
        .saturating_sub(viewport_h as usize);

    let mut effective_offset = if let Some(target) = target_offset {
        dv_node.smooth_scroll.resolve_target(
            old_offset,
            target,
            max_offset,
            dv_node.scroll_behavior,
        )
    } else if let Some(forced) = current_override {
        let offset = forced.min(max_offset);
        dv_node.smooth_scroll.cancel_at(offset);
        offset
    } else {
        let offset = old_offset.min(max_offset);
        dv_node.smooth_scroll.cancel_at(offset);
        offset
    };

    effective_offset = effective_offset.min(max_offset);
    if let Some(anchor_source_line) = scroll_anchor_source_line {
        if let Some(anchored) = dv_node
            .visual_cache
            .source_line_map
            .iter()
            .position(|&source_line| source_line == anchor_source_line)
        {
            effective_offset = anchored.min(max_offset);
            dv_node.smooth_scroll.cancel_at(effective_offset);
        }
    }
    dv_node.scroll_offset = effective_offset;
    dv_node.scroll_override = if current_override.is_some() {
        Some(effective_offset)
    } else {
        None
    };

    // ── Horizontal scroll offset ────────────────────────────────────────
    if h_scrollbar_visible {
        let max_h_offset = (dv_node.max_line_width as usize).saturating_sub(content_w as usize);
        // Preserve existing h_scroll from drag override or previous state
        let h_off = old_h_override.unwrap_or(old_h_offset);
        let h_off = h_off.min(max_h_offset);
        dv_node.h_scroll_offset = h_off;
        dv_node.h_scroll_override = if old_h_override.is_some() {
            Some(h_off)
        } else {
            None
        };
    } else {
        dv_node.h_scroll_offset = 0;
        dv_node.h_scroll_override = None;
    }

    // ── Store in tree ───────────────────────────────────────────────────
    let node = tree.node_mut(id);
    node.rect = rect;
    node.children.clear();
    node.kind = NodeKind::DocumentView(Box::new(dv_node));

    tree.register_scrollbar_zone(id);

    id
}

/// Snap a byte offset to the nearest valid char boundary, rounding down.
fn snap_to_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }

    let mut i = idx;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "diff-view")]
    use std::sync::Arc;
    use std::time::Duration;

    use crate::animation::{Easing, TransitionConfig};
    use crate::core::element::Element;
    use crate::core::node::{NodeKind, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::{Length, Rect};
    #[cfg(feature = "diff-view")]
    use crate::widgets::HStack;
    use crate::widgets::document_view::node::DocumentViewNode;
    use crate::widgets::internal::ScrollAction;
    use crate::widgets::{DocumentView, ScrollBehavior, VStack};

    #[cfg(feature = "diff-view")]
    fn peer_lines(value: &str) -> Arc<Vec<Arc<str>>> {
        Arc::new(value.split('\n').map(Arc::from).collect())
    }

    fn linear_smooth(duration_ms: u64) -> ScrollBehavior {
        ScrollBehavior::smooth(TransitionConfig {
            duration: Duration::from_millis(duration_ms),
            easing: Easing::Linear,
        })
    }

    fn numbered_lines(count: usize) -> String {
        (0..count)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn reconcile_document(
        element: DocumentView,
        width: u16,
        height: u16,
    ) -> (NodeTree, crate::core::node::NodeId) {
        let root: Element = element.into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: width,
                h: height,
            },
            None,
        );
        let root_id = tree.root;
        (tree, root_id)
    }

    fn document_node(tree: &NodeTree, id: crate::core::node::NodeId) -> &DocumentViewNode {
        let NodeKind::DocumentView(node) = &tree.node(id).kind else {
            panic!("expected document view");
        };
        node
    }

    fn document_node_mut(
        tree: &mut NodeTree,
        id: crate::core::node::NodeId,
    ) -> &mut DocumentViewNode {
        let NodeKind::DocumentView(node) = &mut tree.node_mut(id).kind else {
            panic!("expected document view");
        };
        node
    }

    #[test]
    fn scroll_to_source_line_instant_preserves_snap_behavior() {
        let (tree, id) = reconcile_document(
            DocumentView::new(numbered_lines(12))
                .border(false)
                .wrap(false)
                .scroll_to_source_line(5),
            20,
            3,
        );

        let node = document_node(&tree, id);
        assert_eq!(node.scroll_offset, 5);
        assert!(!node.smooth_scroll.is_animating());
    }

    #[test]
    fn smooth_source_line_target_starts_without_snapping() {
        let (tree, id) = reconcile_document(
            DocumentView::new(numbered_lines(12))
                .border(false)
                .wrap(false)
                .scroll_to_source_line(8)
                .scroll_behavior(linear_smooth(100)),
            20,
            3,
        );

        let node = document_node(&tree, id);
        assert_eq!(node.scroll_offset, 0);
        assert!(node.smooth_scroll.is_animating());
    }

    #[test]
    fn smooth_source_line_target_ticks_without_restarting() {
        let element = DocumentView::new(numbered_lines(20))
            .border(false)
            .wrap(false)
            .scroll_to_source_line(10)
            .scroll_transition(TransitionConfig {
                duration: Duration::from_millis(100),
                easing: Easing::Linear,
            });
        let root: Element = element.clone().into();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 2,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);
        let id = tree.root;
        document_node_mut(&mut tree, id)
            .smooth_scroll
            .tick(Duration::from_millis(50), 18);

        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);
        assert_eq!(document_node(&tree, id).scroll_offset, 5);

        document_node_mut(&mut tree, id)
            .smooth_scroll
            .tick(Duration::from_millis(40), 18);
        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);

        assert_eq!(document_node(&tree, id).scroll_offset, 9);
    }

    #[test]
    fn scroll_to_source_line_uses_first_wrapped_visual_line() {
        let (tree, id) = reconcile_document(
            DocumentView::new("alpha beta gamma delta\ntarget\nlast")
                .border(false)
                .wrap(true)
                .scroll_to_source_line(1),
            8,
            2,
        );

        let node = document_node(&tree, id);
        let expected = node
            .visual_cache
            .lines
            .iter()
            .position(|line| line.source_line >= 1)
            .unwrap_or(0)
            .min(node.total_visual_lines.saturating_sub(2));
        assert!(expected > 0, "first source line should wrap before target");
        assert_eq!(node.scroll_offset, expected);
    }

    #[test]
    fn smooth_source_line_target_clamps_after_content_shrinks() {
        let first = DocumentView::new(numbered_lines(20))
            .border(false)
            .wrap(false)
            .scroll_to_source_line(15)
            .scroll_behavior(linear_smooth(100));
        let second = DocumentView::new("only\ntwo")
            .border(false)
            .wrap(false)
            .scroll_to_source_line(15)
            .scroll_behavior(linear_smooth(100));
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 3,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &first.into(), viewport, None);
        let id = tree.root;
        assert!(document_node(&tree, id).smooth_scroll.is_animating());

        LayoutEngine::reconcile_with_focus(&mut tree, &second.into(), viewport, None);

        let node = document_node(&tree, id);
        assert_eq!(node.scroll_offset, 0);
        assert_eq!(node.smooth_scroll.current_offset(0), 0);
    }

    #[test]
    fn user_scroll_suppresses_same_smooth_source_line_target() {
        let element = DocumentView::new(numbered_lines(20))
            .border(false)
            .wrap(false)
            .scroll_to_source_line(10)
            .scroll_behavior(linear_smooth(100));
        let root: Element = element.clone().into();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 3,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);
        let id = tree.root;
        assert!(document_node(&tree, id).smooth_scroll.is_animating());

        assert!(crate::app::input::handlers::document_view::handle_scroll(
            &mut tree,
            id,
            ScrollAction::LineDown(1),
        ));
        assert_eq!(document_node(&tree, id).scroll_offset, 1);

        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);

        let node = document_node(&tree, id);
        assert_eq!(node.scroll_offset, 1);
        assert!(!node.smooth_scroll.is_animating());
    }

    #[test]
    fn noop_user_scroll_suppresses_same_smooth_source_line_target() {
        let element = DocumentView::new(numbered_lines(20))
            .border(false)
            .wrap(false)
            .scroll_to_source_line(10)
            .scroll_behavior(linear_smooth(100));
        let root: Element = element.clone().into();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 3,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);
        let id = tree.root;
        assert_eq!(document_node(&tree, id).scroll_offset, 0);
        assert!(document_node(&tree, id).smooth_scroll.is_animating());

        assert!(crate::app::input::handlers::document_view::handle_scroll(
            &mut tree,
            id,
            ScrollAction::LineUp(1),
        ));
        assert_eq!(document_node(&tree, id).scroll_offset, 0);

        LayoutEngine::reconcile_with_focus(&mut tree, &root, viewport, None);

        let node = document_node(&tree, id);
        assert_eq!(node.scroll_offset, 0);
        assert!(!node.smooth_scroll.is_animating());
        assert_eq!(node.cancelled_scroll_to_source_line, Some(10));
    }

    #[test]
    fn controlled_scroll_offset_cancels_smooth_source_line_target_immediately() {
        let target = DocumentView::new(numbered_lines(20))
            .border(false)
            .wrap(false)
            .scroll_to_source_line(10)
            .scroll_behavior(linear_smooth(100));
        let controlled = target.clone().scroll_offset(4);
        let controlled_root: Element = controlled.into();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 3,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &target.into(), viewport, None);
        let id = tree.root;
        assert!(document_node(&tree, id).smooth_scroll.is_animating());

        LayoutEngine::reconcile_with_focus(&mut tree, &controlled_root, viewport, None);

        let node = document_node(&tree, id);
        assert_eq!(node.scroll_offset, 4);
        assert_eq!(node.scroll_override, Some(4));
        assert_eq!(node.cancelled_scroll_to_source_line, Some(10));
        assert!(!node.smooth_scroll.is_animating());

        LayoutEngine::reconcile_with_focus(&mut tree, &controlled_root, viewport, None);

        let node = document_node(&tree, id);
        assert_eq!(node.scroll_offset, 4);
        assert_eq!(node.scroll_override, Some(4));
        assert_eq!(node.cancelled_scroll_to_source_line, Some(10));
        assert!(!node.smooth_scroll.is_animating());
    }

    #[test]
    fn controlled_scroll_offset_assertion_cancels_smooth_source_line_target() {
        let target = DocumentView::new(numbered_lines(20))
            .border(false)
            .wrap(false)
            .scroll_to_source_line(10)
            .scroll_behavior(linear_smooth(100));
        let controlled_root: Element = target.clone().scroll_offset(0).into();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 3,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &target.into(), viewport, None);
        let id = tree.root;
        assert_eq!(document_node(&tree, id).scroll_offset, 0);
        assert!(document_node(&tree, id).smooth_scroll.is_animating());

        LayoutEngine::reconcile_with_focus(&mut tree, &controlled_root, viewport, None);

        let node = document_node(&tree, id);
        assert_eq!(node.scroll_offset, 0);
        assert_eq!(node.scroll_override, Some(0));
        assert_eq!(node.cancelled_scroll_to_source_line, Some(10));
        assert!(!node.smooth_scroll.is_animating());
    }

    #[test]
    fn auto_height_includes_standalone_horizontal_scrollbar_row() {
        let root: Element = VStack::new()
            .width(Length::Auto)
            .height(Length::Auto)
            .child(
                DocumentView::new("123456789\nabc")
                    .width(Length::Px(5))
                    .height(Length::Auto)
                    .wrap(false)
                    .scrollbar(false)
                    .h_scrollbar(true)
                    .border(false),
            )
            .into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 20,
            },
            None,
        );

        let child_id = tree.node(tree.root).children[0];
        let NodeKind::DocumentView(node) = &tree.node(child_id).kind else {
            panic!("expected document view root");
        };

        assert_eq!(tree.node(child_id).rect.h, 3);
        assert!(node.h_scrollbar);
    }

    #[test]
    fn visual_cache_hits_on_repeat_reconcile_with_identical_inputs() {
        let build_root = || -> Element {
            VStack::new()
                .width(Length::Px(40))
                .height(Length::Auto)
                .child(
                    DocumentView::new("line one\nline two\nline three")
                        .width(Length::Px(40))
                        .height(Length::Auto)
                        .wrap(false)
                        .border(false),
                )
                .into()
        };
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &build_root(), viewport, None);
        let child_id = tree.node(tree.root).children[0];
        let first_key = {
            let NodeKind::DocumentView(node) = &tree.node(child_id).kind else {
                panic!("expected document view");
            };
            node.visual_cache.key
        };
        assert!(first_key.is_some(), "first reconcile must populate key");

        LayoutEngine::reconcile_with_focus(&mut tree, &build_root(), viewport, None);
        let child_id = tree.node(tree.root).children[0];
        let NodeKind::DocumentView(node) = &tree.node(child_id).kind else {
            panic!("expected document view");
        };
        assert_eq!(
            node.visual_cache.key, first_key,
            "second reconcile with identical inputs must reuse the cached key"
        );
    }

    #[cfg(feature = "diff-view")]
    #[test]
    fn split_wrap_document_views_hit_visual_cache_on_repeat_reconcile() {
        let build_root = || -> Element {
            let sync = crate::widgets::diff_view::new_split_wrap_sync_state();
            let left_value = "alpha beta gamma delta epsilon zeta eta theta\nshort";
            let right_value = "alpha\nbeta gamma delta epsilon zeta eta theta iota kappa lambda";

            let mut left = DocumentView::new(left_value)
                .width(Length::Flex(1))
                .height(Length::Auto)
                .wrap(true)
                .border(false)
                .scrollbar(false);
            left.peer_source_lines = Some(peer_lines(right_value));
            left.split_wrap_sync = Some(sync.clone());
            left.split_wrap_side = Some(crate::widgets::diff_view::SplitPaneSide::Left);

            let mut right = DocumentView::new(right_value)
                .width(Length::Flex(1))
                .height(Length::Auto)
                .wrap(true)
                .border(false)
                .scrollbar(false);
            right.peer_source_lines = Some(peer_lines(left_value));
            right.split_wrap_sync = Some(sync);
            right.split_wrap_side = Some(crate::widgets::diff_view::SplitPaneSide::Right);

            HStack::new()
                .width(Length::Px(40))
                .height(Length::Auto)
                .child(left)
                .child(right)
                .into()
        };
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 20,
        };

        let mut tree = NodeTree::new();
        super::reset_visual_cache_stats();
        LayoutEngine::reconcile_with_focus(&mut tree, &build_root(), viewport, None);
        let (_, first_misses, first_ineligible) = super::visual_cache_stats();
        assert_eq!(
            first_ineligible, 0,
            "split-wrap passes should be cache-eligible"
        );
        assert!(
            first_misses >= 4,
            "first dual-pass reconcile should populate caches"
        );

        super::reset_visual_cache_stats();
        LayoutEngine::reconcile_with_focus(&mut tree, &build_root(), viewport, None);
        let (hits, misses, ineligible) = super::visual_cache_stats();
        assert_eq!(
            ineligible, 0,
            "split-wrap repeat should stay cache-eligible"
        );
        assert_eq!(
            misses, 0,
            "two-slot cache should preserve pass 1 and pass 2"
        );
        assert!(
            hits >= 4,
            "repeat dual-pass reconcile should hit both panes and passes"
        );
    }
}
