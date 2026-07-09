//! Scroll-related layout logic for stack.

use rustc_hash::FxHashMap;

use crate::core::element::{Element, Key};
use crate::layout::axis::{Axis, align_x, requested_cross_axis, requested_main_axis};
use crate::layout::hash::element_layout_hash;
use crate::layout::measure::min_size_constrained;
use crate::style::{Align, Length, Rect};
use crate::widgets::internal::StackProps;
use crate::widgets::{VirtualChildEntry, VirtualHeightCache};

use super::types::ScrollContentLayout;
use super::{is_portal, zero_rect};

pub(crate) fn scroll_rect_gaps_after(children: &[Element], rects: &[Rect]) -> Vec<bool> {
    let mut gaps = vec![false; rects.len()];
    let mut has_later_visible_child = false;

    for index in (0..rects.len()).rev() {
        let visible = children.get(index).is_some_and(|child| !is_portal(child))
            && rects.get(index).is_some_and(|rect| rect.h > 0);

        gaps[index] = visible && has_later_visible_child;
        if visible {
            has_later_visible_child = true;
        }
    }

    gaps
}

pub(crate) fn scroll_content_width_from_rects(rects: &[Rect]) -> u16 {
    rects
        .iter()
        .filter(|r| r.w > 0 && r.h > 0)
        .map(|r| r.x.saturating_add(r.w as i16).max(0) as u32)
        .max()
        .unwrap_or(0)
        .min(u16::MAX as u32) as u16
}

pub(crate) fn make_scroll_content_layout(
    rects: Vec<Rect>,
    content_height: u16,
) -> ScrollContentLayout {
    ScrollContentLayout {
        content_width: scroll_content_width_from_rects(&rects),
        rects,
        content_height,
    }
}

pub(crate) fn position_scroll_content_rects(
    children: &[Element],
    rects: &mut [Rect],
    gap: u16,
) -> u16 {
    let gaps = scroll_rect_gaps_after(children, rects);
    let mut y: i16 = 0;

    for (index, (child, rect)) in children.iter().zip(rects.iter_mut()).enumerate() {
        if is_portal(child) {
            *rect = zero_rect(0, 0);
            continue;
        }

        rect.y = y;
        y = y.saturating_add(rect.h as i16);
        if gaps.get(index).copied().unwrap_or(false) {
            y = y.saturating_add(gap as i16);
        }
    }

    y.max(0) as u16
}

fn virtual_entry_has_later_visible_child(
    children: &[Element],
    cache: &VirtualHeightCache,
    start: usize,
) -> bool {
    children
        .iter()
        .enumerate()
        .skip(start)
        .any(|(index, child)| {
            if is_portal(child) {
                return false;
            }

            cache
                .entries
                .get(index)
                .and_then(|entry| entry.as_ref())
                .is_none_or(|entry| !entry.is_portal && entry.h > 0)
        })
}

fn refresh_virtual_gap_after(children: &[Element], cache: &mut VirtualHeightCache) {
    let gaps: Vec<bool> = children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            if is_portal(child) {
                return false;
            }

            cache
                .entries
                .get(index)
                .and_then(|entry| entry.as_ref())
                .is_some_and(|entry| {
                    !entry.is_portal
                        && entry.h > 0
                        && virtual_entry_has_later_visible_child(
                            children,
                            cache,
                            index.saturating_add(1),
                        )
                })
        })
        .collect();

    for (entry, gap_after) in cache.entries.iter_mut().zip(gaps) {
        if let Some(entry) = entry {
            entry.gap_after = gap_after;
        }
    }
}

pub(crate) fn layout_scroll_content(
    props: &StackProps,
    children: &[Element],
    viewport_width: u16,
    viewport_height: u16,
    horizontal_overflow: bool,
) -> ScrollContentLayout {
    if children.is_empty() {
        return make_scroll_content_layout(Vec::new(), 0);
    }

    let non_portal_count = children.iter().filter(|child| !is_portal(child)).count();

    if non_portal_count == 0 {
        return make_scroll_content_layout(children.iter().map(|_| zero_rect(0, 0)).collect(), 0);
    }

    let gap = props.gap;
    let mut out = Vec::with_capacity(children.len());

    for child in children.iter() {
        if is_portal(child) {
            out.push(zero_rect(0, 0));
            continue;
        }

        let mut w = match requested_cross_axis(child, Axis::Vertical) {
            Length::Px(px) => {
                if horizontal_overflow {
                    px
                } else {
                    px.min(viewport_width)
                }
            }
            Length::Percent(percent) => {
                let resolved = Length::Percent(percent).resolve(viewport_width, 0);
                if horizontal_overflow {
                    resolved
                } else {
                    resolved.min(viewport_width)
                }
            }
            Length::Auto => 0,
            Length::Flex(_) => {
                if horizontal_overflow {
                    0
                } else {
                    viewport_width
                }
            }
        };

        if props.align == Align::Stretch && !horizontal_overflow {
            w = viewport_width;
        }

        let measure_max_w = if horizontal_overflow {
            None
        } else {
            Some(if w > 0 { w } else { viewport_width })
        };
        let (cw, ch) = min_size_constrained(child, measure_max_w, None);

        if w == 0 {
            w = if horizontal_overflow {
                cw
            } else {
                cw.min(viewport_width)
            };
        }

        let layout = child.layout_constraints();
        let resolved_min_h = layout.min_h.resolve_as_min(viewport_height);

        let natural_h = match requested_main_axis(child, Axis::Vertical, None) {
            Length::Px(px) => px.max(resolved_min_h),
            Length::Percent(percent) => Length::Percent(percent)
                .resolve(viewport_height, ch)
                .max(resolved_min_h),
            Length::Auto | Length::Flex(_) => ch.max(resolved_min_h),
        };

        let bounds = Rect {
            x: 0,
            y: 0,
            w: viewport_width,
            h: 0,
        };
        let x = align_x(bounds, w, props.align);

        out.push(Rect {
            x,
            y: 0,
            w,
            h: natural_h,
        });
    }

    let content_height = position_scroll_content_rects(children, &mut out, gap);

    make_scroll_content_layout(out, content_height)
}

/// Activation threshold: use virtual measurement only for scroll views
/// with more children than this.
pub(crate) const VIRTUAL_THRESHOLD: usize = 8;

pub(crate) struct ScrollVirtualLayoutParams {
    pub viewport_width: u16,
    pub viewport_height: u16,
    pub scroll_offset: usize,
    pub estimated_child_height: u16,
    pub horizontal_overflow: bool,
}

/// Virtual measurement: only measures visible + buffer children.
/// Off-screen unchanged children reuse their cached heights; unmeasured
/// children use the running average (or `estimated_child_height` on cold start).
pub(crate) fn layout_scroll_content_virtual(
    props: &StackProps,
    children: &[Element],
    cache: &mut VirtualHeightCache,
    params: ScrollVirtualLayoutParams,
) -> ScrollContentLayout {
    let ScrollVirtualLayoutParams {
        viewport_width,
        viewport_height,
        scroll_offset,
        estimated_child_height,
        horizontal_overflow,
    } = params;
    if children.is_empty() {
        cache.reset();
        return make_scroll_content_layout(Vec::new(), 0);
    }

    let non_portal_count = children.iter().filter(|child| !is_portal(child)).count();

    if non_portal_count == 0 {
        cache.reset();
        return make_scroll_content_layout(children.iter().map(|_| zero_rect(0, 0)).collect(), 0);
    }

    // --- 2a. Key-based cache migration (index shifting protection) ---
    migrate_cache_by_key(children, cache);

    // --- 2b. Width change handling ---
    if cache.viewport_w != viewport_width && !cache.entries.is_empty() {
        if cache.width_sensitive {
            // Full reset so measured_count becomes 0. The caller's
            // cold-start bypass will then run a full layout pass
            // that measures every child at the new width and re-seeds
            // the cache - giving exact heights instead of estimates.
            cache.reset();
        } else {
            for entry in cache.entries.iter_mut().flatten() {
                refresh_entry_width(entry, viewport_width, props.align, horizontal_overflow);
            }
        }
        cache.viewport_w = viewport_width;
    } else if cache.entries.is_empty() {
        cache.viewport_w = viewport_width;
    }

    // Ensure entries vector matches children count.
    cache.entries.resize_with(children.len(), || None);

    // --- 2c. Per-child hash check (invalidate stale entries) ---
    for (i, child) in children.iter().enumerate() {
        if is_portal(child) {
            continue;
        }
        let child_hash = element_layout_hash(child);
        if let Some(entry) = cache.entries[i].as_mut()
            && child_hash.is_some_and(|h| h != entry.layout_hash)
        {
            entry.stale = true;
        }
    }
    refresh_virtual_gap_after(children, cache);

    let gap = props.gap;

    // --- Fast path: if every entry is populated, compute rects directly
    //     from the cache without any measurement. This guarantees stable
    //     content_height across LayoutOnly reconciles.
    if cache.entries.len() == children.len()
        && cache
            .entries
            .iter()
            .all(|entry| entry.as_ref().is_some_and(|entry| !entry.stale))
    {
        let mut rects = Vec::with_capacity(children.len());
        for (i, child) in children.iter().enumerate() {
            if is_portal(child) {
                rects.push(zero_rect(0, 0));
                continue;
            }
            let entry = cache.entries[i].as_ref().unwrap();
            rects.push(Rect {
                x: entry.x,
                y: 0,
                w: entry.w,
                h: entry.h,
            });
        }
        let content_height = position_scroll_content_rects(children, &mut rects, gap);
        cache.width_sensitive = children
            .iter()
            .any(crate::widgets::scroll_child_height_depends_on_width);
        return make_scroll_content_layout(rects, content_height);
    }

    // --- 2d. First pass: estimate cumulative Y positions ---
    let range_start = scroll_offset as i16;
    let range_end = range_start.saturating_add(viewport_height as i16);
    let buffer = viewport_height as i16;
    let buffered_start = range_start.saturating_sub(buffer);
    let buffered_end = range_end.saturating_add(buffer);

    if scroll_offset > 0 {
        premeasure_stale_prefix_before_viewport(
            children,
            props,
            cache,
            PremeasureStalePrefixParams {
                viewport_width,
                viewport_height,
                non_portal_count,
                estimated_child_height,
                before_y: buffered_start,
                horizontal_overflow,
            },
        );
    }

    let estimate = cache.estimated_height(estimated_child_height);

    // Combined buffer-detection + measurement pass.
    //
    // We walk children in order, tracking a running `y` from the most
    // up-to-date cache entries. When a stale or absent entry falls within
    // the buffered visible zone we measure it before advancing `y`, so a
    // single inflated stale height cannot push later in-buffer items past
    // `buffered_end` (the previous decoupled passes computed `cumulative_y`
    // before measuring, which silently dropped re-measurements for the
    // items immediately after a stale-too-big neighbor).
    let mut y: i16 = 0;
    let mut layout_idx = 0usize;
    for (i, child) in children.iter().enumerate() {
        if is_portal(child) {
            // Ensure portal entries are set correctly.
            if let Some(old) = cache.entries[i].take()
                && !old.is_portal
            {
                cache.unrecord_measurement(old.h);
            }
            cache.entries[i] = Some(VirtualChildEntry {
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
            });
            continue;
        }

        let entry_h = cache.entries[i].as_ref().map(|e| e.h).unwrap_or(estimate);
        let child_bottom = y.saturating_add(entry_h as i16);
        let in_buffer = child_bottom > buffered_start && y < buffered_end;

        let child_hash = element_layout_hash(child);
        let is_unhashable = child_hash.is_none();
        let is_stale = cache.entries[i].as_ref().is_some_and(|entry| entry.stale);

        let needs_measure = cache.entries[i].is_none()
            || is_stale
            || (is_unhashable && in_buffer && child_bottom > range_start && y < range_end);

        if needs_measure && in_buffer {
            measure_and_cache_child(
                child,
                i,
                props,
                cache,
                MeasureCacheChildParams {
                    viewport_width,
                    viewport_height,
                    horizontal_overflow,
                },
            );
        }

        let h = cache.entries[i].as_ref().map(|e| e.h).unwrap_or(estimate);
        let gap_after = cache.entries[i]
            .as_ref()
            .map(|entry| entry.gap_after)
            .unwrap_or(layout_idx < non_portal_count.saturating_sub(1));
        y = y.saturating_add(h as i16);
        if gap_after {
            y = y.saturating_add(gap as i16);
        }
        layout_idx += 1;
    }

    // --- 2g. Recompute cumulative Y with corrected heights ---
    let estimate = cache.estimated_height(estimated_child_height);
    // Best-guess width for off-screen children when scrolling horizontally,
    // so `Auto`/`Flex` children contribute their natural size to the content
    // width instead of collapsing to 0 (mirrors the height estimate above).
    let width_estimate = if horizontal_overflow {
        cache.estimated_width()
    } else {
        0
    };
    let mut rects = Vec::with_capacity(children.len());

    for (i, child) in children.iter().enumerate() {
        if is_portal(child) {
            rects.push(zero_rect(0, 0));
            continue;
        }

        let (w, h, x) = if let Some(entry) = &cache.entries[i] {
            (entry.w, entry.h, entry.x)
        } else {
            // Off-screen unmeasured: use estimate for height, compute width.
            let w = resolve_child_width(
                child,
                viewport_width,
                props.align,
                horizontal_overflow,
                width_estimate,
            );
            let bounds = Rect {
                x: 0,
                y: 0,
                w: viewport_width,
                h: 0,
            };
            let x = align_x(bounds, w, props.align);
            (w, estimate, x)
        };

        rects.push(Rect { x, y: 0, w, h });
    }

    refresh_virtual_gap_after(children, cache);
    let content_height = position_scroll_content_rects(children, &mut rects, gap);

    // Update width_sensitive flag for future frames.
    cache.width_sensitive = children
        .iter()
        .any(crate::widgets::scroll_child_height_depends_on_width);

    make_scroll_content_layout(rects, content_height)
}

/// Resolve the width a child wants without measuring it.
///
/// When `horizontal_overflow` is set, content-sized children (`Auto`/`Flex`)
/// cannot be resolved without measuring, so they fall back to `width_estimate`
/// (the running average of measured children) rather than collapsing to 0.
fn resolve_child_width(
    child: &Element,
    viewport_width: u16,
    align: Align,
    horizontal_overflow: bool,
    width_estimate: u16,
) -> u16 {
    let w = match requested_cross_axis(child, Axis::Vertical) {
        Length::Px(px) => {
            if horizontal_overflow {
                px
            } else {
                px.min(viewport_width)
            }
        }
        Length::Percent(percent) => {
            let resolved = Length::Percent(percent).resolve(viewport_width, 0);
            if horizontal_overflow {
                resolved
            } else {
                resolved.min(viewport_width)
            }
        }
        Length::Auto => {
            if horizontal_overflow {
                width_estimate
            } else {
                viewport_width
            }
        }
        Length::Flex(_) => {
            if horizontal_overflow {
                width_estimate
            } else {
                viewport_width
            }
        }
    };
    if align == Align::Stretch && !horizontal_overflow {
        viewport_width
    } else {
        w
    }
}

struct MeasureCacheChildParams {
    viewport_width: u16,
    viewport_height: u16,
    horizontal_overflow: bool,
}

/// Measure a single child and store the result in the virtual cache.
fn measure_and_cache_child(
    child: &Element,
    idx: usize,
    props: &StackProps,
    cache: &mut VirtualHeightCache,
    params: MeasureCacheChildParams,
) {
    let MeasureCacheChildParams {
        viewport_width,
        viewport_height,
        horizontal_overflow,
    } = params;
    let mut w = match requested_cross_axis(child, Axis::Vertical) {
        Length::Px(px) => {
            if horizontal_overflow {
                px
            } else {
                px.min(viewport_width)
            }
        }
        Length::Percent(percent) => {
            let resolved = Length::Percent(percent).resolve(viewport_width, 0);
            if horizontal_overflow {
                resolved
            } else {
                resolved.min(viewport_width)
            }
        }
        Length::Auto => 0,
        Length::Flex(_) => {
            if horizontal_overflow {
                0
            } else {
                viewport_width
            }
        }
    };

    if props.align == Align::Stretch && !horizontal_overflow {
        w = viewport_width;
    }

    let measure_max_w = if horizontal_overflow {
        None
    } else {
        Some(if w > 0 { w } else { viewport_width })
    };
    let (cw, ch) = min_size_constrained(child, measure_max_w, None);

    if w == 0 {
        w = if horizontal_overflow {
            cw
        } else {
            cw.min(viewport_width)
        };
    }

    let layout = child.layout_constraints();
    let resolved_min_h = layout.min_h.resolve_as_min(viewport_height);

    let natural_h = match requested_main_axis(child, Axis::Vertical, None) {
        Length::Px(px) => px.max(resolved_min_h),
        Length::Percent(percent) => Length::Percent(percent)
            .resolve(viewport_height, ch)
            .max(resolved_min_h),
        Length::Auto | Length::Flex(_) => ch.max(resolved_min_h),
    };

    let bounds = Rect {
        x: 0,
        y: 0,
        w: viewport_width,
        h: 0,
    };
    let x = align_x(bounds, w, props.align);

    let requested_w = requested_cross_axis(child, Axis::Vertical);
    let flex_w = !horizontal_overflow && matches!(requested_w, Length::Flex(_));
    let percent_w = match requested_w {
        Length::Percent(percent) => Some(percent.min(100)),
        _ => None,
    };

    let child_hash = element_layout_hash(child).unwrap_or(0);

    // If replacing an existing entry, unrecord the old measurement first.
    if let Some(old) = &cache.entries[idx]
        && !old.is_portal
    {
        cache.unrecord_measurement(old.h);
    }

    cache.entries[idx] = Some(VirtualChildEntry {
        layout_hash: child_hash,
        key: child.key.clone(),
        h: natural_h,
        w,
        x,
        flex_w,
        percent_w,
        gap_after: false,
        is_portal: false,
        stale: false,
    });
    cache.record_measurement(natural_h);
}

struct PremeasureStalePrefixParams {
    viewport_width: u16,
    viewport_height: u16,
    non_portal_count: usize,
    estimated_child_height: u16,
    before_y: i16,
    horizontal_overflow: bool,
}

fn premeasure_stale_prefix_before_viewport(
    children: &[Element],
    props: &StackProps,
    cache: &mut VirtualHeightCache,
    params: PremeasureStalePrefixParams,
) {
    let PremeasureStalePrefixParams {
        viewport_width,
        viewport_height,
        non_portal_count,
        estimated_child_height,
        before_y,
        horizontal_overflow,
    } = params;
    if before_y <= 0 {
        return;
    }

    let mut y: i16 = 0;
    let mut layout_idx = 0usize;
    let estimate = cache.estimated_height(estimated_child_height);

    for (i, child) in children.iter().enumerate() {
        if is_portal(child) {
            continue;
        }

        let h = cache.entries[i]
            .as_ref()
            .map(|entry| entry.h)
            .unwrap_or(estimate);
        let gap_after = cache.entries[i]
            .as_ref()
            .map(|entry| entry.gap_after)
            .unwrap_or(layout_idx < non_portal_count.saturating_sub(1) && h > 0);

        let child_bottom = y.saturating_add(h as i16);
        if child_bottom > before_y {
            break;
        }

        let needs_measure = cache.entries[i].is_none()
            || cache.entries[i].as_ref().is_some_and(|entry| entry.stale);
        if needs_measure {
            measure_and_cache_child(
                child,
                i,
                props,
                cache,
                MeasureCacheChildParams {
                    viewport_width,
                    viewport_height,
                    horizontal_overflow,
                },
            );
        }

        let resolved_h = cache.entries[i].as_ref().map(|entry| entry.h).unwrap_or(h);
        y = y.saturating_add(resolved_h as i16);
        if gap_after {
            y = y.saturating_add(props.gap as i16);
        }
        layout_idx += 1;
    }
}

/// Key-based cache migration: when children are added/removed, match old
/// cache entries to new children by their `Key` to survive index shifts.
fn refresh_entry_width(
    entry: &mut VirtualChildEntry,
    viewport_width: u16,
    align: Align,
    horizontal_overflow: bool,
) {
    if entry.is_portal {
        entry.w = 0;
        entry.x = 0;
        return;
    }

    let w = if horizontal_overflow {
        entry.w
    } else if align == Align::Stretch || entry.flex_w {
        viewport_width
    } else if let Some(percent) = entry.percent_w {
        Length::Percent(percent).resolve(viewport_width, 0)
    } else {
        entry.w.min(viewport_width)
    };

    entry.w = w;
    entry.x = align_x(
        Rect {
            x: 0,
            y: 0,
            w: viewport_width,
            h: 0,
        },
        w,
        align,
    );
}

/// After a ScrollView layout-cache shortcut (Tier 1/2) updates the content
/// viewport width without running [`layout_scroll_content_virtual`], cached
/// per-child widths can still reflect a **previous** width (e.g. standalone
/// scrollbar narrow pass). Refresh them so the virtual fast path cannot emit
/// stale rects on the next frame.
pub(crate) fn sync_virtual_cache_entry_widths(
    cache: &mut VirtualHeightCache,
    viewport_w: u16,
    align: Align,
    horizontal_overflow: bool,
) {
    if cache.entries.is_empty() {
        cache.viewport_w = viewport_w;
        return;
    }
    for entry in cache.entries.iter_mut().flatten() {
        refresh_entry_width(entry, viewport_w, align, horizontal_overflow);
    }
    cache.viewport_w = viewport_w;
}

fn migrate_cache_by_key(children: &[Element], cache: &mut VirtualHeightCache) {
    if cache.entries.is_empty() {
        return;
    }

    let old_entries = std::mem::take(&mut cache.entries);
    let mut by_key: FxHashMap<Key, VirtualChildEntry> = FxHashMap::default();
    for entry in old_entries.iter().flatten() {
        if let Some(key) = &entry.key {
            by_key.insert(key.clone(), entry.clone());
        }
    }

    let mut new_entries = Vec::with_capacity(children.len());
    let mut total_h: u64 = 0;
    let mut count: u32 = 0;

    for (i, child) in children.iter().enumerate() {
        let child_hash = element_layout_hash(child);
        let positional = old_entries
            .get(i)
            .and_then(|entry| entry.as_ref())
            .and_then(|entry| {
                (entry.key.is_none()
                    && child.key.is_none()
                    && child_hash == Some(entry.layout_hash))
                .then(|| entry.clone())
            });
        let entry = child
            .key
            .as_ref()
            .and_then(|key| by_key.remove(key))
            .or(positional);

        if let Some(e) = &entry
            && !e.is_portal
        {
            total_h += e.h as u64;
            count += 1;
        }
        new_entries.push(entry);
    }

    cache.entries = new_entries;
    cache.total_measured_height = total_h;
    cache.measured_count = count;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::element::{Element, ElementKind};
    use crate::overlay::{DismissPolicy, OverlayLayer, OverlayPlacement, PointerCapture, Portal};
    use crate::widgets::Text;

    /// Helper: build a `Text` element with explicit Px height and Px width.
    fn text_px(w: u16, h: u16) -> Element {
        Text::new("x")
            .width(Length::Px(w))
            .height(Length::Px(h))
            .into()
    }

    /// Helper: build a `Text` element with custom Length values.
    fn text_len(width: Length, height: Length) -> Element {
        Text::new("x").width(width).height(height).into()
    }

    /// Helper: build a portal element (filtered out during layout).
    fn portal_element() -> Element {
        let inner = Text::new("hidden").into();
        Element::new(ElementKind::Portal(Portal {
            layer: OverlayLayer::Modal,
            content: Box::new(inner),
            placement: OverlayPlacement::Center {
                reserve_height: None,
            },
            dismiss_policy: DismissPolicy::None,
            on_close: None,
            backdrop: None,
            captures_focus: false,
            captures_pointer: PointerCapture::None,
        }))
    }

    fn default_props() -> StackProps {
        StackProps::default()
    }

    // ---------------------------------------------------------------
    // 1. Empty children → empty result
    // ---------------------------------------------------------------
    #[test]
    fn empty_children_returns_empty_layout() {
        let props = default_props();
        let result = layout_scroll_content(&props, &[], 80, 24, false);
        assert!(result.rects.is_empty());
        assert_eq!(result.content_height, 0);
    }

    // ---------------------------------------------------------------
    // 2. Natural height accumulation with gap
    //    Three children each 5px tall, gap=2 → total = 5+2+5+2+5 = 19
    // ---------------------------------------------------------------
    #[test]
    fn content_height_accumulates_with_gaps() {
        let mut props = default_props();
        props.gap = 2;
        let children = vec![text_px(20, 5), text_px(20, 5), text_px(20, 5)];

        let result = layout_scroll_content(&props, &children, 40, 24, false);

        // y positions: 0, 5+2=7, 7+5+2=14
        assert_eq!(result.rects[0].y, 0);
        assert_eq!(result.rects[0].h, 5);
        assert_eq!(result.rects[1].y, 7);
        assert_eq!(result.rects[1].h, 5);
        assert_eq!(result.rects[2].y, 14);
        assert_eq!(result.rects[2].h, 5);
        // total: 14 + 5 = 19 (no trailing gap after last child)
        assert_eq!(result.content_height, 19);
    }

    // ---------------------------------------------------------------
    // 3. Portal children are filtered out and get zero-sized rects
    // ---------------------------------------------------------------
    #[test]
    fn portals_filtered_and_get_zero_rects() {
        let props = default_props();
        let children = vec![text_px(20, 3), portal_element(), text_px(20, 7)];

        let result = layout_scroll_content(&props, &children, 40, 24, false);

        assert_eq!(result.rects.len(), 3);

        // First child laid out normally at y=0.
        assert_eq!(result.rects[0].y, 0);
        assert_eq!(result.rects[0].h, 3);

        // Portal gets a zero-sized rect.
        assert_eq!(
            result.rects[1],
            Rect {
                x: 0,
                y: 0,
                w: 0,
                h: 0
            }
        );

        // Third child follows the first (no gap since props.gap=0).
        assert_eq!(result.rects[2].y, 3);
        assert_eq!(result.rects[2].h, 7);

        // Content height = 3 + 7 = 10.
        assert_eq!(result.content_height, 10);
    }

    // ---------------------------------------------------------------
    // 4. Cross-axis alignment: Center and End within viewport width
    // ---------------------------------------------------------------
    #[test]
    fn cross_axis_alignment_center_and_end() {
        let viewport_width = 40;

        // Center alignment: child w=10 → x = (40-10)/2 = 15
        let mut props = default_props();
        props.align = Align::Center;
        let children = vec![text_px(10, 3)];
        let result = layout_scroll_content(&props, &children, viewport_width, 24, false);
        assert_eq!(result.rects[0].x, 15);
        assert_eq!(result.rects[0].w, 10);

        // End alignment: child w=10 → x = 40-10 = 30
        props.align = Align::End;
        let result = layout_scroll_content(&props, &children, viewport_width, 24, false);
        assert_eq!(result.rects[0].x, 30);
        assert_eq!(result.rects[0].w, 10);
    }

    #[test]
    fn percent_width_resolves_against_viewport_width() {
        let props = default_props();
        let children = vec![text_len(Length::Percent(50), Length::Px(3))];

        let result = layout_scroll_content(&props, &children, 40, 20, false);

        assert_eq!(result.rects[0].w, 20);
        assert_eq!(result.rects[0].h, 3);
    }

    #[test]
    fn percent_height_resolves_against_viewport_height() {
        let props = default_props();
        let children = vec![text_len(Length::Px(10), Length::Percent(50))];

        let result = layout_scroll_content(&props, &children, 40, 20, false);

        assert_eq!(result.rects[0].h, 10);
        assert_eq!(result.content_height, 10);
    }

    // ---------------------------------------------------------------
    // 5. Single child: content height equals child height, no gap
    // ---------------------------------------------------------------
    #[test]
    fn single_child_no_trailing_gap() {
        let mut props = default_props();
        props.gap = 5; // gap should NOT appear after a single child
        let children = vec![text_px(20, 8)];

        let result = layout_scroll_content(&props, &children, 40, 24, false);

        assert_eq!(result.rects.len(), 1);
        assert_eq!(result.rects[0].y, 0);
        assert_eq!(result.rects[0].h, 8);
        assert_eq!(result.content_height, 8);
    }

    #[test]
    fn zero_height_trailing_child_does_not_contribute_gap() {
        let mut props = default_props();
        props.gap = 1;
        let children = vec![text_px(20, 1), text_px(20, 0).key("bottom")];

        let result = layout_scroll_content(&props, &children, 40, 24, false);

        assert_eq!(result.rects[0].y, 0);
        assert_eq!(result.rects[0].h, 1);
        assert_eq!(result.rects[1].y, 1);
        assert_eq!(result.rects[1].h, 0);
        assert_eq!(result.content_height, 1);
    }

    #[test]
    fn zero_height_middle_child_preserves_single_gap_between_visible_children() {
        let mut props = default_props();
        props.gap = 1;
        let children = vec![text_px(20, 1), text_px(20, 0), text_px(20, 1)];

        let result = layout_scroll_content(&props, &children, 40, 24, false);

        assert_eq!(result.rects[0].y, 0);
        assert_eq!(result.rects[1].y, 2);
        assert_eq!(result.rects[1].h, 0);
        assert_eq!(result.rects[2].y, 2);
        assert_eq!(result.content_height, 3);
    }

    #[test]
    fn virtual_layout_migrates_keyed_entries_after_insert_at_top() {
        let props = default_props();
        let mut cache = VirtualHeightCache::default();
        let children = vec![
            text_px(10, 2).key("a"),
            text_px(10, 3).key("b"),
            text_px(10, 4).key("c"),
        ];

        let first = layout_scroll_content_virtual(
            &props,
            &children,
            &mut cache,
            ScrollVirtualLayoutParams {
                viewport_width: 20,
                viewport_height: 4,
                scroll_offset: 0,
                estimated_child_height: 3,
                horizontal_overflow: false,
            },
        );
        assert_eq!(first.content_height, 9);
        assert_eq!(cache.measured_count, 3);

        let inserted = vec![
            text_px(10, 5).key("new"),
            text_px(10, 2).key("a"),
            text_px(10, 3).key("b"),
            text_px(10, 4).key("c"),
        ];
        let second = layout_scroll_content_virtual(
            &props,
            &inserted,
            &mut cache,
            ScrollVirtualLayoutParams {
                viewport_width: 20,
                viewport_height: 4,
                scroll_offset: 0,
                estimated_child_height: 3,
                horizontal_overflow: false,
            },
        );

        assert_eq!(second.rects[1].h, 2);
        assert_eq!(second.rects[2].h, 3);
        assert_eq!(second.rects[3].h, 4);
        assert_eq!(cache.measured_count, 4);
    }

    #[test]
    fn virtual_layout_uses_running_average_for_offscreen_unmeasured_children() {
        let props = default_props();
        let mut cache = VirtualHeightCache::default();
        let children = vec![
            text_px(10, 2).key("a"),
            text_px(10, 4).key("b"),
            text_px(10, 10).key("c"),
            text_px(10, 10).key("d"),
        ];

        let result = layout_scroll_content_virtual(
            &props,
            &children,
            &mut cache,
            ScrollVirtualLayoutParams {
                viewport_width: 20,
                viewport_height: 2,
                scroll_offset: 0,
                estimated_child_height: 3,
                horizontal_overflow: false,
            },
        );

        assert_eq!(cache.measured_count, 2);
        assert_eq!(cache.estimated_height(3), 3);
        assert_eq!(result.rects[2].h, 3);
        assert_eq!(result.rects[3].h, 3);
        assert_eq!(result.content_height, 12);
    }

    #[test]
    fn virtual_layout_estimates_offscreen_auto_width_under_horizontal_overflow() {
        // "wide cell" is 9 columns wide; with horizontal overflow an Auto-width
        // child keeps its natural width instead of being clamped to the viewport.
        let auto_text = |h: u16| -> Element {
            Text::new("wide cell")
                .width(Length::Auto)
                .height(Length::Px(h))
                .into()
        };
        let props = default_props();
        let mut cache = VirtualHeightCache::default();
        let children = vec![
            auto_text(2).key("a"),
            auto_text(4).key("b"),
            auto_text(10).key("c"),
            auto_text(10).key("d"),
        ];

        // viewport_width 4 (< 9) with horizontal_overflow=true; viewport_height 2
        // so children c/d stay off-screen and unmeasured.
        let result = layout_scroll_content_virtual(
            &props,
            &children,
            &mut cache,
            ScrollVirtualLayoutParams {
                viewport_width: 4,
                viewport_height: 2,
                scroll_offset: 0,
                estimated_child_height: 3,
                horizontal_overflow: true,
            },
        );

        // Only a/b are measured, each at its natural width of 9.
        assert_eq!(cache.measured_count, 2);
        assert_eq!(result.rects[0].w, 9);
        assert_eq!(cache.estimated_width(), 9);

        // Off-screen Auto children fall back to the width estimate (9), not 0,
        // so the content width reflects them (regression: previously collapsed to 0).
        assert_eq!(result.rects[2].w, 9);
        assert_eq!(result.rects[3].w, 9);
        assert_eq!(result.content_width, 9);
    }

    #[test]
    fn virtual_layout_refreshes_cached_widths_without_remeasurement_when_stable() {
        let props = default_props();
        let mut cache = VirtualHeightCache::default();
        let children = vec![
            text_len(Length::Percent(50), Length::Px(2)).key("a"),
            text_len(Length::Percent(50), Length::Px(2)).key("b"),
            text_len(Length::Percent(50), Length::Px(2)).key("c"),
            text_len(Length::Percent(50), Length::Px(2)).key("d"),
            text_len(Length::Percent(50), Length::Px(2)).key("e"),
            text_len(Length::Percent(50), Length::Px(2)).key("f"),
            text_len(Length::Percent(50), Length::Px(2)).key("g"),
            text_len(Length::Percent(50), Length::Px(2)).key("h"),
            text_len(Length::Percent(50), Length::Px(2)).key("i"),
        ];

        let wide = layout_scroll_content_virtual(
            &props,
            &children,
            &mut cache,
            ScrollVirtualLayoutParams {
                viewport_width: 20,
                viewport_height: 3,
                scroll_offset: 0,
                estimated_child_height: 2,
                horizontal_overflow: false,
            },
        );
        let measured_before = cache.measured_count;
        let narrow = layout_scroll_content_virtual(
            &props,
            &children,
            &mut cache,
            ScrollVirtualLayoutParams {
                viewport_width: 10,
                viewport_height: 3,
                scroll_offset: 0,
                estimated_child_height: 2,
                horizontal_overflow: false,
            },
        );

        assert_eq!(measured_before, cache.measured_count);
        assert_eq!(wide.rects[0].w, 10);
        assert_eq!(narrow.rects[0].w, 5);
        assert_eq!(narrow.rects[0].h, 2);
    }

    #[test]
    fn sync_virtual_cache_entry_widths_updates_stale_flex_rows() {
        use crate::style::Align;

        let mut cache = VirtualHeightCache {
            viewport_w: 80,
            entries: vec![Some(VirtualChildEntry {
                layout_hash: 1,
                key: None,
                h: 3,
                w: 70,
                x: 0,
                flex_w: true,
                percent_w: None,
                gap_after: false,
                is_portal: false,
                stale: false,
            })],
            ..Default::default()
        };

        sync_virtual_cache_entry_widths(&mut cache, 100, Align::Start, false);

        let e = cache.entries[0].as_ref().unwrap();
        assert_eq!(cache.viewport_w, 100);
        assert_eq!(e.w, 100);
        assert_eq!(e.x, 0);
    }

    #[test]
    fn virtual_layout_keeps_offscreen_stale_rows_cached_until_needed() {
        let props = default_props();
        let mut cache = VirtualHeightCache::default();
        let children: Vec<_> = (0..9)
            .map(|i| text_len(Length::Px(10), Length::Px(2)).key(format!("row-{i}")))
            .collect();

        let first = layout_scroll_content_virtual(
            &props,
            &children,
            &mut cache,
            ScrollVirtualLayoutParams {
                viewport_width: 20,
                viewport_height: 18,
                scroll_offset: 0,
                estimated_child_height: 2,
                horizontal_overflow: false,
            },
        );
        assert_eq!(first.content_height, 18);
        assert!(cache.entries.iter().flatten().all(|entry| !entry.stale));

        for entry in cache.entries.iter_mut().flatten() {
            entry.stale = true;
        }

        let second = layout_scroll_content_virtual(
            &props,
            &children,
            &mut cache,
            ScrollVirtualLayoutParams {
                viewport_width: 20,
                viewport_height: 2,
                scroll_offset: 0,
                estimated_child_height: 2,
                horizontal_overflow: false,
            },
        );

        assert_eq!(second.content_height, 18);
        assert!(cache.entries[0].as_ref().is_some_and(|entry| !entry.stale));
        assert!(cache.entries[1].as_ref().is_some_and(|entry| !entry.stale));
        assert!(cache.entries[2].as_ref().is_some_and(|entry| entry.stale));
        assert!(cache.entries[8].as_ref().is_some_and(|entry| entry.stale));
    }

    #[test]
    fn virtual_layout_remeasures_visible_items_after_preceding_stale_too_big_entry() {
        // Reproduces an opencode-tui scroll-view bug: when an in-buffer stale
        // entry's cached `h` is larger than the new height it would measure to,
        // the inflated `cumulative_y` falsely placed later in-buffer items
        // beyond `buffered_end`. Those items kept their stale heights and
        // rendered with extra empty rows below their actual content.
        let props = default_props();
        let mut cache = VirtualHeightCache::default();

        // 12 children of height 5 - populates the virtual cache initially.
        let initial: Vec<_> = (0..12)
            .map(|i| text_len(Length::Px(10), Length::Px(5)).key(format!("row-{i}")))
            .collect();
        let _ = layout_scroll_content_virtual(
            &props,
            &initial,
            &mut cache,
            ScrollVirtualLayoutParams {
                viewport_width: 20,
                viewport_height: 30,
                scroll_offset: 0,
                estimated_child_height: 5,
                horizontal_overflow: false,
            },
        );
        assert_eq!(cache.measured_count, 12);

        // Swap the children: same keys, but heights now 1 (much smaller).
        // migrate_cache_by_key keeps the old entries; the per-child hash check
        // marks them stale. All 12 should re-measure to h=1 in subsequent
        // virtual passes that touch them.
        let swapped: Vec<_> = (0..12)
            .map(|i| text_len(Length::Px(10), Length::Px(1)).key(format!("row-{i}")))
            .collect();

        // Viewport 30 covers all real rows (12 × 1 = 12) but the cached
        // heights inflate `cumulative_y` to 60, pushing later items past the
        // buffered_end (≈ 90 here, generous) only when buffer is small.
        // Use a tight viewport to expose the issue.
        let viewport_h = 6;
        let result = layout_scroll_content_virtual(
            &props,
            &swapped,
            &mut cache,
            ScrollVirtualLayoutParams {
                viewport_width: 20,
                viewport_height: viewport_h,
                scroll_offset: 0,
                estimated_child_height: 5,
                horizontal_overflow: false,
            },
        );

        // Every visible/buffered row's cache entry should reflect the new size.
        // Without the fix, later in-buffer rows kept h=5.
        for (i, entry) in cache.entries.iter().enumerate() {
            let entry = entry.as_ref().expect("entry exists");
            // Items whose true position falls within the buffered zone must
            // be re-measured. With viewport_h=6 and gap=0 the buffered zone
            // is roughly y ∈ [-6, 12], so all 12 rows of new height 1 fit.
            assert_eq!(
                entry.h, 1,
                "row {i} should be re-measured to its new height (was stale at h=5)"
            );
        }
        assert_eq!(result.content_height, 12);
    }
}
