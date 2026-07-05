use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::core::element::{Element, ElementKind, Key};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::axis::{Axis, requested_cross_axis};
use crate::layout::hash::{element_layout_hash, layout_hasher};
use crate::layout::stack::{
    ScrollContentLayout, ScrollVirtualLayoutParams, VIRTUAL_THRESHOLD, layout_scroll_content,
    layout_scroll_content_virtual, make_scroll_content_layout, scroll_rect_gaps_after,
    sync_virtual_cache_entry_widths,
};
use crate::style::{Length, Rect};
use crate::widgets::scroll_view::node::SingleDocSelection;
use crate::widgets::{HeightCacheItem, ScrollTarget, ScrollViewLayoutCache, VirtualHeightCache};

use super::recompute_from_height_cache;
pub(crate) static SCROLL_LAYOUT_CACHE_CALLS: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCROLL_LAYOUT_CACHE_SPLIT_WRAP_CALLS: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCROLL_LAYOUT_CACHE_EXACT_HITS: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCROLL_LAYOUT_CACHE_SPLIT_WRAP_EXACT_HITS: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCROLL_LAYOUT_CACHE_EXACT_MISSES: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCROLL_LAYOUT_CACHE_UNHASHABLE: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCROLL_LAYOUT_CACHE_HEIGHT_HITS: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCROLL_LAYOUT_CACHE_VIRTUAL_RUNS: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCROLL_LAYOUT_CACHE_FULL_LAYOUTS: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCROLL_LAYOUT_CACHE_SPLIT_WRAP_FULL_LAYOUTS: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCROLL_LAYOUT_CACHE_SYNC_DRIFTS: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCROLL_LAYOUT_CACHE_INVALIDATIONS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn maybe_log_scroll_layout_cache_stats() {
    use std::sync::OnceLock;

    static ENABLED: OnceLock<bool> = OnceLock::new();
    let enabled = *ENABLED.get_or_init(|| std::env::var_os("TUI_LIPAN_CACHE_STATS").is_some());
    if !enabled {
        return;
    }

    let calls = SCROLL_LAYOUT_CACHE_CALLS.load(Ordering::Relaxed);
    if calls == 0 || calls % 500 != 0 {
        return;
    }

    let split_calls = SCROLL_LAYOUT_CACHE_SPLIT_WRAP_CALLS.load(Ordering::Relaxed);
    let exact_hits = SCROLL_LAYOUT_CACHE_EXACT_HITS.load(Ordering::Relaxed);
    let split_exact_hits = SCROLL_LAYOUT_CACHE_SPLIT_WRAP_EXACT_HITS.load(Ordering::Relaxed);
    let exact_misses = SCROLL_LAYOUT_CACHE_EXACT_MISSES.load(Ordering::Relaxed);
    let unhashable = SCROLL_LAYOUT_CACHE_UNHASHABLE.load(Ordering::Relaxed);
    let height_hits = SCROLL_LAYOUT_CACHE_HEIGHT_HITS.load(Ordering::Relaxed);
    let virtual_runs = SCROLL_LAYOUT_CACHE_VIRTUAL_RUNS.load(Ordering::Relaxed);
    let full_layouts = SCROLL_LAYOUT_CACHE_FULL_LAYOUTS.load(Ordering::Relaxed);
    let split_full_layouts = SCROLL_LAYOUT_CACHE_SPLIT_WRAP_FULL_LAYOUTS.load(Ordering::Relaxed);
    let sync_drifts = SCROLL_LAYOUT_CACHE_SYNC_DRIFTS.load(Ordering::Relaxed);
    let invalidations = SCROLL_LAYOUT_CACHE_INVALIDATIONS.load(Ordering::Relaxed);
    let exact_attempts = exact_hits + exact_misses;
    let exact_hit_rate = if exact_attempts > 0 {
        (exact_hits as f64 / exact_attempts as f64) * 100.0
    } else {
        0.0
    };

    eprintln!(
        "[tui-lipan scroll_layout_cache] calls={calls} split_calls={split_calls} exact_hits={exact_hits} split_exact_hits={split_exact_hits} exact_misses={exact_misses} unhashable={unhashable} height_hits={height_hits} virtual_runs={virtual_runs} full_layouts={full_layouts} split_full_layouts={split_full_layouts} sync_drifts={sync_drifts} invalidations={invalidations} exact_hit_rate={exact_hit_rate:.1}%"
    );
}
/// Walk a node's subtree depth-first and collect ALL `DocumentView` selections
/// (preserving order so they can be matched back on restore).
pub(crate) fn collect_doc_selections_in_subtree(
    tree: &mut NodeTree,
    node_id: NodeId,
    out: &mut Vec<SingleDocSelection>,
) {
    if !tree.is_valid(node_id) {
        return;
    }
    if matches!(tree.node(node_id).kind, NodeKind::DocumentView(_)) {
        let NodeKind::DocumentView(doc) = &mut tree.node_mut(node_id).kind else {
            return;
        };
        let format_cache = std::mem::take(&mut doc.format_cache);
        let visual_cache = std::mem::take(&mut doc.visual_cache);
        let flat_text = visual_cache.flat_text.clone();
        out.push(SingleDocSelection {
            selection_cursor: doc.selection_cursor,
            selection_anchor: doc.selection_anchor,
            table_rect_selection: doc.table_rect_selection.take(),
            format_cache,
            visual_cache,
            flat_text,
            shared_selection_id: doc.shared_selection_id.clone(),
        });
        return; // DocumentViews don't nest
    }
    let children = tree.node(node_id).children.clone();
    for child_id in &children {
        collect_doc_selections_in_subtree(tree, *child_id, out);
    }
}

pub(crate) fn scroll_offset_for_key(
    children: &[Element],
    rects: &[Rect],
    target: &Key,
) -> Option<usize> {
    children.iter().zip(rects.iter()).find_map(|(child, rect)| {
        element_subtree_contains_key(child, target).then_some(rect.y.max(0) as usize)
    })
}

pub(crate) fn scroll_offset_for_target(
    children: &[Element],
    rects: &[Rect],
    target: &ScrollTarget,
    max_offset: usize,
) -> Option<usize> {
    match target {
        ScrollTarget::Top => Some(0),
        ScrollTarget::Bottom => Some(max_offset),
        ScrollTarget::Key(key) => scroll_offset_for_key(children, rects, key),
        ScrollTarget::KeyOffset { key, offset } => scroll_offset_for_key(children, rects, key)
            .map(|base| base.saturating_add(*offset).min(max_offset)),
    }
}

fn element_subtree_contains_key(element: &Element, target: &Key) -> bool {
    if element.key.as_ref() == Some(target) {
        return true;
    }

    element
        .kind
        .children()
        .iter()
        .any(|child| element_subtree_contains_key(child, target))
}

pub(crate) struct ScrollLayoutCachedParams {
    pub viewport_w: u16,
    pub viewport_h: u16,
    pub scroll_offset: usize,
    pub estimated_child_height: u16,
    pub horizontal_overflow: bool,
}

pub(crate) fn layout_scroll_content_cached(
    props: &crate::widgets::internal::StackProps,
    children: &[crate::core::element::Element],
    cache: &mut ScrollViewLayoutCache,
    virtual_cache: &mut VirtualHeightCache,
    params: ScrollLayoutCachedParams,
) -> ScrollContentLayout {
    let ScrollLayoutCachedParams {
        viewport_w,
        viewport_h,
        scroll_offset,
        estimated_child_height,
        horizontal_overflow,
    } = params;
    SCROLL_LAYOUT_CACHE_CALLS.fetch_add(1, Ordering::Relaxed);

    let viewport_h_affects_layout = children
        .iter()
        .any(scroll_child_height_depends_on_scroll_viewport_h);
    #[cfg(feature = "diff-view")]
    let has_split_wrap_sync = children
        .iter()
        .any(crate::widgets::element_subtree_has_split_wrap_sync);
    #[cfg(not(feature = "diff-view"))]
    let has_split_wrap_sync = false;
    if has_split_wrap_sync {
        SCROLL_LAYOUT_CACHE_SPLIT_WRAP_CALLS.fetch_add(1, Ordering::Relaxed);
    }
    let hashes = scroll_content_hashes(
        props,
        children,
        viewport_w,
        viewport_h,
        viewport_h_affects_layout,
        has_split_wrap_sync,
    );
    let width_sensitive_children = children.iter().any(scroll_child_height_depends_on_width);
    if hashes.is_none() {
        SCROLL_LAYOUT_CACHE_UNHASHABLE.fetch_add(1, Ordering::Relaxed);
    }
    cache.active_content_hash = hashes.as_ref().map(|hashes| hashes.content_hash_no_width);

    // Tier 1: exact cache hit (same viewport width + same content). This is
    // safe for split-wrap rows because the stored rects came from a full layout
    // at the same width; width-changing shortcuts below stay disabled for them.
    if let Some(hash) = hashes.as_ref().map(|hashes| hashes.layout_hash)
        && let Some((rects, content_height)) = cache.get(viewport_w, hash, children.len())
    {
        SCROLL_LAYOUT_CACHE_EXACT_HITS.fetch_add(1, Ordering::Relaxed);
        if has_split_wrap_sync {
            SCROLL_LAYOUT_CACHE_SPLIT_WRAP_EXACT_HITS.fetch_add(1, Ordering::Relaxed);
        }
        maybe_log_scroll_layout_cache_stats();
        // Keep viewport_w in sync so the anchor-correction logic on the
        // next frame sees the correct "old" width even when a probe pass
        // already populated the layout cache at this width.
        let layout = make_scroll_content_layout(rects, content_height);
        if should_seed_virtual_cache_from_exact(children.len(), virtual_cache) {
            populate_virtual_cache_from_layout(
                children,
                &layout,
                viewport_w,
                width_sensitive_children,
                virtual_cache,
            );
        } else {
            sync_virtual_cache_entry_widths(
                virtual_cache,
                viewport_w,
                props.align,
                horizontal_overflow,
            );
        }
        return layout;
    }
    if hashes.is_some() {
        SCROLL_LAYOUT_CACHE_EXACT_MISSES.fetch_add(1, Ordering::Relaxed);
    }

    // Tier 2: height cache hit (same content, different viewport width).
    // Avoids the expensive min_size_constrained calls when only width changes.
    let content_hash = hashes.as_ref().map(|hashes| hashes.content_hash_no_width);
    if !has_split_wrap_sync
        && let Some(ch) = content_hash
        && let Some(result) =
            recompute_from_height_cache(props, viewport_w, cache, ch, horizontal_overflow)
    {
        SCROLL_LAYOUT_CACHE_HEIGHT_HITS.fetch_add(1, Ordering::Relaxed);
        if let Some(hash) = hashes.as_ref().map(|hashes| hashes.layout_hash) {
            cache.insert(
                viewport_w,
                hash,
                result.rects.clone(),
                result.content_height,
            );
        }
        if should_seed_virtual_cache_from_exact(children.len(), virtual_cache) {
            populate_virtual_cache_from_layout(
                children,
                &result,
                viewport_w,
                width_sensitive_children,
                virtual_cache,
            );
        } else {
            sync_virtual_cache_entry_widths(
                virtual_cache,
                viewport_w,
                props.align,
                horizontal_overflow,
            );
        }
        maybe_log_scroll_layout_cache_stats();
        return result;
    }

    // Width-sensitive resize detection: if the virtual cache was built at a
    // different width and the content is width-sensitive, reset the cache so
    // we fall through to a full layout pass that measures every child.
    if children.len() > VIRTUAL_THRESHOLD
        && virtual_cache.measured_count > 0
        && virtual_cache.viewport_w != viewport_w
        && !virtual_cache.entries.is_empty()
        && virtual_cache.width_sensitive
    {
        virtual_cache.reset();
    }

    // Virtual scrolling substitutes averages for unmeasured rows; split-wrapped
    // DiffView panes are height-unstable until both measure passes have run at
    // the final widths, so always use the full stack measure path instead.
    if children.len() > VIRTUAL_THRESHOLD
        && virtual_cache.measured_count > 0
        && !has_split_wrap_sync
    {
        SCROLL_LAYOUT_CACHE_VIRTUAL_RUNS.fetch_add(1, Ordering::Relaxed);
        let layout = layout_scroll_content_virtual(
            props,
            children,
            virtual_cache,
            ScrollVirtualLayoutParams {
                viewport_width: viewport_w,
                viewport_height: viewport_h,
                scroll_offset,
                estimated_child_height,
                horizontal_overflow,
            },
        );

        if virtual_cache.entries.len() == children.len()
            && virtual_cache
                .entries
                .iter()
                .all(|entry| entry.as_ref().is_some_and(|entry| !entry.stale))
        {
            if let Some(hash) = hashes.as_ref().map(|hashes| hashes.layout_hash) {
                cache.insert(
                    viewport_w,
                    hash,
                    layout.rects.clone(),
                    layout.content_height,
                );
            }
            if let Some(ch) = content_hash {
                let items = build_height_cache_items(children, &layout);
                cache.store_heights(ch, width_sensitive_children, items);
            }
        }

        maybe_log_scroll_layout_cache_stats();
        return layout;
    }

    // Full layout pass.
    SCROLL_LAYOUT_CACHE_FULL_LAYOUTS.fetch_add(1, Ordering::Relaxed);
    if has_split_wrap_sync {
        SCROLL_LAYOUT_CACHE_SPLIT_WRAP_FULL_LAYOUTS.fetch_add(1, Ordering::Relaxed);
    }
    let layout =
        layout_scroll_content(props, children, viewport_w, viewport_h, horizontal_overflow);
    if let Some(hash) = hashes.as_ref().map(|hashes| hashes.layout_hash) {
        cache.insert(
            viewport_w,
            hash,
            layout.rects.clone(),
            layout.content_height,
        );
    }

    // Populate height cache for future width-only changes.
    if let Some(ch) = content_hash {
        let items = build_height_cache_items(children, &layout);
        cache.store_heights(ch, width_sensitive_children, items);
    }

    // Seed the virtual cache from the full layout so subsequent frames
    // can use the virtual (partial measurement) path.
    if children.len() > VIRTUAL_THRESHOLD {
        populate_virtual_cache_from_layout(
            children,
            &layout,
            viewport_w,
            width_sensitive_children,
            virtual_cache,
        );
    }

    maybe_log_scroll_layout_cache_stats();
    layout
}

fn should_seed_virtual_cache_from_exact(
    children_len: usize,
    virtual_cache: &VirtualHeightCache,
) -> bool {
    children_len > VIRTUAL_THRESHOLD
        && (virtual_cache.entries.len() != children_len
            || virtual_cache
                .entries
                .iter()
                .any(|entry| entry.as_ref().is_none_or(|entry| entry.stale)))
}

pub(crate) struct ScrollContentHashes {
    pub(crate) layout_hash: u64,
    pub(crate) content_hash_no_width: u64,
}

pub(crate) fn scroll_content_hashes(
    props: &crate::widgets::internal::StackProps,
    children: &[crate::core::element::Element],
    viewport_w: u16,
    viewport_h: u16,
    viewport_h_affects_layout: bool,
    has_split_wrap_sync: bool,
) -> Option<ScrollContentHashes> {
    let mut layout_hash = layout_hasher();
    let mut content_hash_no_width = layout_hasher();

    viewport_w.hash(&mut layout_hash);
    if viewport_h_affects_layout {
        viewport_h.hash(&mut layout_hash);
        viewport_h.hash(&mut content_hash_no_width);
    }
    props.gap.hash(&mut layout_hash);
    props.gap.hash(&mut content_hash_no_width);
    props.align.hash(&mut layout_hash);
    props.align.hash(&mut content_hash_no_width);
    has_split_wrap_sync.hash(&mut layout_hash);
    has_split_wrap_sync.hash(&mut content_hash_no_width);

    for child in children {
        let child_hash = element_layout_hash(child)?;
        child_hash.hash(&mut layout_hash);
        child_hash.hash(&mut content_hash_no_width);
    }

    Some(ScrollContentHashes {
        layout_hash: layout_hash.finish(),
        content_hash_no_width: content_hash_no_width.finish(),
    })
}

/// Build height cache items from a freshly computed layout.
pub(crate) fn build_height_cache_items(
    children: &[crate::core::element::Element],
    layout: &ScrollContentLayout,
) -> Vec<HeightCacheItem> {
    let gaps = scroll_rect_gaps_after(children, &layout.rects);
    let mut items = Vec::with_capacity(children.len());

    for (index, (child, rect)) in children.iter().zip(layout.rects.iter()).enumerate() {
        if matches!(child.kind, ElementKind::Portal(_)) {
            items.push(HeightCacheItem {
                natural_w: 0,
                h: 0,
                flex_w: false,
                percent_w: None,
                gap_after: false,
                is_portal: true,
            });
            continue;
        }

        let requested_w = requested_cross_axis(child, Axis::Vertical);
        let flex_w = matches!(requested_w, Length::Flex(_));
        let percent_w = match requested_w {
            Length::Percent(percent) => Some(percent.min(100)),
            _ => None,
        };
        let gap_after = gaps.get(index).copied().unwrap_or(false);

        items.push(HeightCacheItem {
            natural_w: rect.w,
            h: rect.h,
            flex_w,
            percent_w,
            gap_after,
            is_portal: false,
        });
    }

    items
}

/// Seed the virtual cache from a full layout pass so that subsequent frames
/// can use the cheaper virtual (partial measurement) path.
pub(crate) fn populate_virtual_cache_from_layout(
    children: &[Element],
    layout: &ScrollContentLayout,
    viewport_w: u16,
    width_sensitive: bool,
    virtual_cache: &mut VirtualHeightCache,
) {
    use crate::layout::axis::{Axis, requested_cross_axis};
    use crate::layout::hash::element_layout_hash;
    use crate::widgets::VirtualChildEntry;

    virtual_cache.reset();
    virtual_cache.viewport_w = viewport_w;
    virtual_cache.width_sensitive = width_sensitive;

    let gaps = scroll_rect_gaps_after(children, &layout.rects);
    let mut entries = Vec::with_capacity(children.len());

    for (index, (child, rect)) in children.iter().zip(layout.rects.iter()).enumerate() {
        if matches!(child.kind, ElementKind::Portal(_)) {
            entries.push(Some(VirtualChildEntry {
                layout_hash: 0,
                key: child.key.clone(),
                h: 0,
                w: 0,
                x: 0,
                flex_w: false,
                percent_w: None,
                gap_after: false,
                is_portal: true,
                stale: false,
            }));
            continue;
        }

        let requested_w = requested_cross_axis(child, Axis::Vertical);
        let flex_w = matches!(requested_w, Length::Flex(_));
        let percent_w = match requested_w {
            Length::Percent(percent) => Some(percent.min(100)),
            _ => None,
        };
        let gap_after = gaps.get(index).copied().unwrap_or(false);

        entries.push(Some(VirtualChildEntry {
            layout_hash: element_layout_hash(child).unwrap_or(0),
            key: child.key.clone(),
            h: rect.h,
            w: rect.w,
            x: rect.x,
            flex_w,
            percent_w,
            gap_after,
            is_portal: false,
            stale: false,
        }));
        virtual_cache.record_measurement(rect.h);
    }

    virtual_cache.entries = entries;
}

pub(crate) fn scroll_child_height_depends_on_width(el: &Element) -> bool {
    use crate::style::Length;
    use crate::widgets::text::Overflow;

    // Handle types whose `dimensions()` returns `None` (Frame, Group,
    // MouseRegion, Popover) first - they need recursive delegation rather
    // than the generic Px/Percent short-circuit below.
    match &el.kind {
        ElementKind::Frame(frame) => {
            if matches!(frame.props.height, Length::Px(_)) {
                return false;
            }
            let child_sensitive = frame
                .child
                .as_deref()
                .is_some_and(scroll_child_height_depends_on_width);
            let header_sensitive = frame
                .header
                .as_deref()
                .is_some_and(scroll_child_height_depends_on_width);
            return child_sensitive || header_sensitive;
        }
        ElementKind::Group(group) => {
            return scroll_child_height_depends_on_width(group.child.as_ref());
        }
        ElementKind::EffectScope(scope) => {
            return scope
                .child
                .as_deref()
                .is_some_and(scroll_child_height_depends_on_width);
        }
        ElementKind::MouseRegion(region) => {
            return region
                .child
                .as_deref()
                .is_some_and(scroll_child_height_depends_on_width);
        }
        ElementKind::Popover(_) => return true,
        ElementKind::Portal(_) => return false,
        ElementKind::Grid(g) => {
            return g
                .items
                .iter()
                .any(|i| scroll_child_height_depends_on_width(&i.element));
        }
        _ => {}
    }

    // For standard widgets, a fixed-pixel or percent height cannot change
    // when only the viewport width changes.
    if let Some((_, height)) = el.kind.dimensions()
        && matches!(height, Length::Px(_) | Length::Percent(_))
    {
        return false;
    }

    match &el.kind {
        ElementKind::Text(text) => {
            matches!(text.overflow, Overflow::Wrap | Overflow::Auto)
        }
        ElementKind::DocumentView(doc) => {
            matches!(doc.resolved_height(), Length::Auto) && (doc.wrap || doc.h_scrollbar)
        }
        ElementKind::TextArea(area) => {
            matches!(area.height, Length::Auto) && (area.wrap || area.h_scrollbar)
        }
        ElementKind::ScrollView(_)
        | ElementKind::VStack(_)
        | ElementKind::HStack(_)
        | ElementKind::Grid(_)
        | ElementKind::Flow(_)
        | ElementKind::ZStack(_)
        | ElementKind::Center(_)
        | ElementKind::CenterPin(_)
        | ElementKind::Splitter(_) => true,
        _ => el
            .kind
            .children()
            .iter()
            .any(|child| scroll_child_height_depends_on_width(child)),
    }
}

/// Returns true if this scroll child's laid-out height can depend on the scroll
/// content viewport **height** (see [`layout_scroll_content`]: percent main-axis
/// height and percent `min_h` both use `viewport_height`).
///
/// When no child satisfies this, scroll layout cache keys can ignore
/// `viewport_h`, so vertical resize reuses Tier-1 / Tier-2 caches instead of
/// forcing a full `min_size_constrained` pass every frame.
pub(crate) fn scroll_child_height_depends_on_scroll_viewport_h(el: &Element) -> bool {
    use crate::layout::axis::{Axis, requested_main_axis};
    use crate::style::Length;

    if matches!(el.layout_constraints().min_h, Length::Percent(_)) {
        return true;
    }
    if matches!(
        requested_main_axis(el, Axis::Vertical, None),
        Length::Percent(_)
    ) {
        return true;
    }

    match &el.kind {
        ElementKind::Frame(frame) => {
            if matches!(frame.props.height, Length::Px(_)) {
                return false;
            }
            frame
                .child
                .as_deref()
                .is_some_and(scroll_child_height_depends_on_scroll_viewport_h)
                || frame
                    .header
                    .as_deref()
                    .is_some_and(scroll_child_height_depends_on_scroll_viewport_h)
        }
        ElementKind::Group(group) => {
            scroll_child_height_depends_on_scroll_viewport_h(group.child.as_ref())
        }
        ElementKind::EffectScope(scope) => scope
            .child
            .as_deref()
            .is_some_and(scroll_child_height_depends_on_scroll_viewport_h),
        ElementKind::MouseRegion(region) => region
            .child
            .as_deref()
            .is_some_and(scroll_child_height_depends_on_scroll_viewport_h),
        ElementKind::Popover(_) => true,
        _ => false,
    }
}
