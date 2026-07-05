use crate::core::element::{Element, Key};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::axis::align_x;
use crate::layout::stack::ScrollContentLayout;
use crate::style::{Length, Rect};
use crate::widgets::internal::{RememberedScrollAnchor, ScrollViewNode};
use crate::widgets::{ScrollViewLayoutCache, VirtualHeightCache};

use crate::widgets::scroll_view::utils::calc_scroll_view_window;

#[derive(Clone)]
struct AnchorCandidate {
    key: Option<Key>,
    index: usize,
    y: i32,
    h: u16,
}

fn usize_to_i32_saturating(value: usize) -> i32 {
    value.min(i32::MAX as usize) as i32
}

fn pinned_anchor(offset: usize, pinned_top: bool, pinned_bottom: bool) -> RememberedScrollAnchor {
    RememberedScrollAnchor {
        top_child_key: None,
        top_child_index: 0,
        top_delta: 0,
        old_offset: offset,
        center_child_key: None,
        center_child_index: 0,
        anchor_numerator: 0,
        anchor_denominator: 1,
        pinned_top,
        pinned_bottom,
    }
}

fn anchor_fraction(anchor_line: i32, child_y: i32, child_h: u16) -> (u32, u32) {
    if child_h == 0 {
        return (0, 1);
    }

    let raw_offset = anchor_line.saturating_sub(child_y);
    let child_span = i32::from(child_h.saturating_sub(1));
    let clamped_offset = raw_offset.clamp(0, child_span) as u32;
    let numerator = clamped_offset.saturating_mul(2).saturating_add(1);
    let denominator = u32::from(child_h).saturating_mul(2).max(1);
    (numerator, denominator)
}

fn select_scroll_anchor<I>(
    candidates: I,
    offset: usize,
    viewport_height: u16,
    pinned_top: bool,
    pinned_bottom: bool,
) -> Option<RememberedScrollAnchor>
where
    I: IntoIterator<Item = AnchorCandidate>,
{
    if pinned_top || pinned_bottom {
        return Some(pinned_anchor(offset, pinned_top, pinned_bottom));
    }

    let offset_i32 = usize_to_i32_saturating(offset);
    let center_line = offset_i32.saturating_add(i32::from(viewport_height / 2));
    let mut top: Option<AnchorCandidate> = None;
    let mut center_containing: Option<AnchorCandidate> = None;
    let mut nearest_center: Option<(AnchorCandidate, i64)> = None;

    for candidate in candidates {
        if candidate.h == 0 {
            continue;
        }

        let child_bottom = candidate.y.saturating_add(i32::from(candidate.h));
        if top.is_none() && child_bottom > offset_i32 {
            top = Some(candidate.clone());
        }

        if center_containing.is_none() && center_line >= candidate.y && center_line < child_bottom {
            center_containing = Some(candidate.clone());
        }

        let child_center = candidate.y.saturating_add(i32::from(candidate.h / 2));
        let distance = (i64::from(child_center) - i64::from(center_line)).abs();
        if nearest_center
            .as_ref()
            .is_none_or(|(_, best_distance)| distance < *best_distance)
        {
            nearest_center = Some((candidate, distance));
        }
    }

    let top = top?;
    let center = center_containing.or_else(|| nearest_center.map(|(candidate, _)| candidate))?;
    let (anchor_numerator, anchor_denominator) = anchor_fraction(center_line, center.y, center.h);

    Some(RememberedScrollAnchor {
        top_child_key: top.key,
        top_child_index: top.index,
        top_delta: offset_i32.saturating_sub(top.y),
        old_offset: offset,
        center_child_key: center.key,
        center_child_index: center.index,
        anchor_numerator,
        anchor_denominator,
        pinned_top: false,
        pinned_bottom: false,
    })
}

pub(crate) fn compute_scroll_anchor(
    cache: &VirtualHeightCache,
    offset: usize,
    viewport_height: u16,
    max_offset: usize,
    estimated_child_height: u16,
    gap: u16,
) -> Option<RememberedScrollAnchor> {
    let pinned_top = offset == 0;
    let pinned_bottom = max_offset > 0 && offset >= max_offset;

    if cache.entries.is_empty() {
        // Top/bottom pinning must work even when the virtual height cache is empty
        // (e.g. `children.len() <= VIRTUAL_THRESHOLD`, so the cache is never seeded).
        return (pinned_top || pinned_bottom)
            .then(|| pinned_anchor(offset, pinned_top, pinned_bottom));
    }

    let estimate = cache.estimated_height(estimated_child_height);
    let mut y = 0i32;
    let candidates = cache.entries.iter().enumerate().map(|(index, entry)| {
        let h = entry
            .as_ref()
            .map(|entry| if entry.is_portal { 0 } else { entry.h })
            .unwrap_or(estimate);
        let gap_after = entry.as_ref().is_some_and(|entry| entry.gap_after);
        let candidate = AnchorCandidate {
            key: entry.as_ref().and_then(|entry| entry.key.clone()),
            index,
            y,
            h,
        };

        y = y.saturating_add(i32::from(h));
        if gap_after {
            y = y.saturating_add(i32::from(gap));
        }

        candidate
    });

    select_scroll_anchor(
        candidates,
        offset,
        viewport_height,
        pinned_top,
        pinned_bottom,
    )
}

fn child_index_for_key(children: &[Element], key: &Key) -> Option<usize> {
    children
        .iter()
        .position(|child| child.key.as_ref() == Some(key))
}

fn snapshot_child_index(
    scroll: &ScrollViewNode,
    key: Option<&Key>,
    content_y: i32,
    visible_ordinal: usize,
) -> Option<usize> {
    let snapshot = scroll.viewport_snapshot.as_ref()?;
    if let Some(visible) = snapshot.visible.get(visible_ordinal)
        && visible.key.as_ref() == key
        && i32::from(visible.content_rect.y) == content_y
    {
        return Some(visible.index);
    }

    snapshot
        .visible
        .iter()
        .find(|visible| {
            visible.key.as_ref() == key && i32::from(visible.content_rect.y) == content_y
        })
        .map(|visible| visible.index)
}

fn virtual_cache_child_index(
    cache: &VirtualHeightCache,
    gap: u16,
    content_y: i32,
) -> Option<usize> {
    let mut y = 0i32;
    for (index, entry) in cache.entries.iter().enumerate() {
        let entry = entry.as_ref()?;
        let h = if entry.is_portal { 0 } else { entry.h };
        if h > 0 && y == content_y {
            return Some(index);
        }

        y = y.saturating_add(i32::from(h));
        if entry.gap_after {
            y = y.saturating_add(i32::from(gap));
        }
    }

    None
}

pub(crate) fn compute_visible_scroll_anchor(
    tree: &NodeTree,
    scroll_id: NodeId,
    children: &[Element],
) -> Option<RememberedScrollAnchor> {
    let node = tree.node(scroll_id);
    let NodeKind::ScrollView(scroll) = &node.kind else {
        return None;
    };

    let pinned_top = scroll.offset == 0;
    let pinned_bottom = scroll.max_offset > 0 && scroll.offset >= scroll.max_offset;
    if pinned_top || pinned_bottom {
        return Some(pinned_anchor(scroll.offset, pinned_top, pinned_bottom));
    }

    let inner = node.rect.inner(scroll.props.border, scroll.props.padding);
    if inner.h == 0 {
        return None;
    }

    let offset_i32 = usize_to_i32_saturating(scroll.offset);
    let top_indicator = if scroll.top_indicator { 1 } else { 0 };
    let content_base_y = i32::from(inner.y).saturating_add(top_indicator);
    let candidates = node
        .children
        .iter()
        .copied()
        .filter(|child_id| tree.is_valid(*child_id))
        .enumerate()
        .filter_map(|(visible_ordinal, child_id)| {
            let child = tree.node(child_id);
            if child.rect.h == 0 {
                return None;
            }

            let content_y = i32::from(child.rect.y)
                .saturating_sub(content_base_y)
                .saturating_add(offset_i32);
            let key = child.key.clone();
            let index = key
                .as_ref()
                .and_then(|key| child_index_for_key(children, key))
                .or_else(|| snapshot_child_index(scroll, key.as_ref(), content_y, visible_ordinal))
                .or_else(|| {
                    virtual_cache_child_index(&scroll.virtual_cache, scroll.props.gap, content_y)
                })?;

            Some(AnchorCandidate {
                key,
                index,
                y: content_y,
                h: child.rect.h,
            })
        });

    select_scroll_anchor(
        candidates,
        scroll.offset,
        inner.h,
        pinned_top,
        pinned_bottom,
    )
}

pub(crate) fn compute_layout_scroll_anchor(
    children: &[Element],
    rects: &[Rect],
    offset: usize,
    viewport_height: u16,
    max_offset: usize,
) -> Option<RememberedScrollAnchor> {
    let pinned_top = offset == 0;
    let pinned_bottom = max_offset > 0 && offset >= max_offset;

    let candidates = rects
        .iter()
        .enumerate()
        .map(|(index, rect)| AnchorCandidate {
            key: children.get(index).and_then(|child| child.key.clone()),
            index,
            y: i32::from(rect.y),
            h: rect.h,
        });

    select_scroll_anchor(
        candidates,
        offset,
        viewport_height,
        pinned_top,
        pinned_bottom,
    )
}

fn rect_for_anchor<'a>(
    children: &[Element],
    rects: &'a [Rect],
    child_key: Option<&Key>,
    child_index: usize,
) -> Option<&'a Rect> {
    match child_key {
        Some(key) => children
            .iter()
            .position(|child| child.key.as_ref() == Some(key))
            .and_then(|idx| rects.get(idx)),
        None => rects.get(child_index),
    }
}

fn clamped_old_offset(
    anchor: &RememberedScrollAnchor,
    content_height: u16,
    viewport_height: u16,
    show_scroll_indicators: bool,
) -> usize {
    let window = calc_scroll_view_window(
        anchor.old_offset,
        content_height as usize,
        viewport_height as usize,
        show_scroll_indicators,
    );
    window.offset.min(window.max_offset)
}

/// Apply an anchor to new layout rects to compute the corrected scroll offset.
pub(crate) fn apply_scroll_anchor(
    children: &[Element],
    rects: &[Rect],
    anchor: &RememberedScrollAnchor,
    content_height: u16,
    viewport_height: u16,
    show_scroll_indicators: bool,
) -> usize {
    let max_offset = calc_scroll_view_window(
        0,
        content_height as usize,
        viewport_height as usize,
        show_scroll_indicators,
    )
    .max_offset;

    if anchor.pinned_top {
        return 0;
    }
    if anchor.pinned_bottom {
        return max_offset;
    }

    let rect = rect_for_anchor(
        children,
        rects,
        anchor.center_child_key.as_ref(),
        anchor.center_child_index,
    );

    if let Some(rect) = rect {
        let anchor_in_viewport = i32::from(viewport_height / 2);
        let child_height = u64::from(rect.h);
        let anchor_numerator = u64::from(anchor.anchor_numerator);
        let anchor_denominator = u64::from(anchor.anchor_denominator.max(1));
        let offset_within_child = if child_height == 0 {
            0i32
        } else {
            ((child_height * anchor_numerator) / anchor_denominator)
                .min(child_height.saturating_sub(1)) as i32
        };
        let new_y = i32::from(rect.y) + offset_within_child - anchor_in_viewport;
        let window = calc_scroll_view_window(
            new_y.max(0) as usize,
            content_height as usize,
            viewport_height as usize,
            show_scroll_indicators,
        );
        window.offset.min(window.max_offset)
    } else {
        clamped_old_offset(
            anchor,
            content_height,
            viewport_height,
            show_scroll_indicators,
        )
    }
}

pub(crate) fn content_change_tail_offset(
    old_offset: usize,
    old_max_offset: usize,
    content_height: u16,
    viewport_height: u16,
    show_scroll_indicators: bool,
    tail_requested: bool,
) -> Option<usize> {
    let max_offset = calc_scroll_view_window(
        0,
        content_height as usize,
        viewport_height as usize,
        show_scroll_indicators,
    )
    .max_offset;

    if (old_max_offset > 0 && old_offset >= old_max_offset)
        || (old_max_offset == 0 && max_offset > 0 && tail_requested)
    {
        Some(max_offset)
    } else {
        None
    }
}

/// Apply an anchor by keeping the old viewport top edge relative to the top child.
///
/// This is for content changes with an unchanged viewport size. A proportional
/// anchor is useful for resize reflow, but when a child grows because streaming
/// content was appended below the reader, keeping the same top edge avoids
/// nudging the viewport while the user is scrolled up.
pub(super) fn apply_scroll_anchor_top_edge(
    children: &[Element],
    rects: &[Rect],
    anchor: &RememberedScrollAnchor,
    content_height: u16,
    viewport_height: u16,
    show_scroll_indicators: bool,
) -> usize {
    let max_offset = calc_scroll_view_window(
        0,
        content_height as usize,
        viewport_height as usize,
        show_scroll_indicators,
    )
    .max_offset;

    if anchor.pinned_top {
        return 0;
    }
    if anchor.pinned_bottom {
        return max_offset;
    }

    let rect = rect_for_anchor(
        children,
        rects,
        anchor.top_child_key.as_ref(),
        anchor.top_child_index,
    );

    if let Some(rect) = rect {
        let new_y = i32::from(rect.y).saturating_add(anchor.top_delta);
        let window = calc_scroll_view_window(
            new_y.max(0) as usize,
            content_height as usize,
            viewport_height as usize,
            show_scroll_indicators,
        );
        window.offset.min(window.max_offset)
    } else {
        clamped_old_offset(
            anchor,
            content_height,
            viewport_height,
            show_scroll_indicators,
        )
    }
}

/// Recompute rects for a new `viewport_w` using cached per-child heights.
/// Returns `None` if the cache is stale or missing.
pub(crate) fn recompute_from_height_cache(
    props: &crate::widgets::internal::StackProps,
    viewport_w: u16,
    cache: &ScrollViewLayoutCache,
    content_hash: u64,
    horizontal_overflow: bool,
) -> Option<ScrollContentLayout> {
    let hc = cache.height_cache.as_ref()?;
    if hc.content_hash != content_hash || hc.width_sensitive {
        return None;
    }

    let gap = props.gap;
    let mut y: i16 = 0;
    let mut rects = Vec::with_capacity(hc.items.len());
    let bounds = Rect {
        x: 0,
        y: 0,
        w: viewport_w,
        h: 0,
    };

    for item in &hc.items {
        if item.is_portal {
            rects.push(Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
            });
            continue;
        }

        let w = if horizontal_overflow {
            if item.flex_w {
                item.natural_w
            } else if let Some(percent) = item.percent_w {
                Length::Percent(percent).resolve(viewport_w, 0)
            } else {
                item.natural_w
            }
        } else if item.flex_w {
            viewport_w
        } else if let Some(percent) = item.percent_w {
            Length::Percent(percent).resolve(viewport_w, 0)
        } else {
            item.natural_w.min(viewport_w)
        };
        let x = align_x(bounds, w, props.align);
        let h = item.h;

        rects.push(Rect { x, y, w, h });
        y = y.saturating_add(h as i16);
        if item.gap_after {
            y = y.saturating_add(gap as i16);
        }
    }

    Some(crate::layout::stack::make_scroll_content_layout(
        rects, y as u16,
    ))
}
