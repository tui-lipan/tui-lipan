use crate::utils::math::round_mul_div;
use crate::utils::scrollbar::ScrollbarMetrics;
use crate::widgets::ListItem;

pub(crate) fn list_item_height(item: &ListItem) -> usize {
    item.line_count().max(1)
}

pub(crate) fn visible_items_for_height(
    items: &[ListItem],
    offset: usize,
    available_lines: u16,
) -> usize {
    if available_lines == 0 || items.is_empty() || offset >= items.len() {
        return 0;
    }

    let mut used = 0usize;
    let mut count = 0usize;
    let budget = available_lines as usize;

    for item in items.iter().skip(offset) {
        let h = list_item_height(item);
        if used + h > budget {
            if count == 0 {
                return 1;
            }
            break;
        }
        used += h;
        count += 1;
    }

    if count == 0 { 1 } else { count }
}

pub(crate) fn max_visible_items_for_height(items: &[ListItem], available_lines: u16) -> usize {
    if available_lines == 0 || items.is_empty() {
        return 0;
    }

    let budget = available_lines as usize;
    let heights: Vec<usize> = items.iter().map(list_item_height).collect();
    let mut best = 0usize;
    let mut end = 0usize;
    let mut used = 0usize;

    for start in 0..items.len() {
        while end < items.len() {
            let h = heights[end].max(1);
            if used + h > budget {
                break;
            }
            used += h;
            end += 1;
        }

        best = best.max(end.saturating_sub(start));

        if start == end {
            end = end.saturating_add(1);
        } else {
            used = used.saturating_sub(heights[start].max(1));
        }
    }

    best.max(1)
}

fn end_for_budget(start: usize, total: usize, heights: &[usize], budget: usize) -> usize {
    if budget == 0 || start >= total {
        return start;
    }

    let mut used = 0usize;
    let mut end = start;

    while end < total {
        let h = heights.get(end).copied().unwrap_or(1).max(1);
        if used + h > budget {
            if end == start {
                return start + 1;
            }
            break;
        }
        used += h;
        end += 1;
    }

    end
}

pub(crate) fn calc_list_window_with_heights(
    offset: usize,
    total: usize,
    max_display: usize,
    heights: &[usize],
) -> (usize, usize, bool, bool) {
    if max_display == 0 || total == 0 {
        return (0, 0, false, false);
    }

    if total <= max_display && heights.iter().take(total).copied().sum::<usize>() <= max_display {
        return (0, total, false, false);
    }

    let start = offset.min(total.saturating_sub(1));
    let has_top = start >= 2;
    let top_reserved = has_top as usize;

    let slots_after_top = max_display.saturating_sub(top_reserved);
    if slots_after_top == 0 {
        return (start, start, has_top, false);
    }

    let budget_with_bottom = slots_after_top.saturating_sub(1);
    let end_with_bottom = end_for_budget(start, total, heights, budget_with_bottom);
    let hidden_below_with_bottom = total.saturating_sub(end_with_bottom);

    if budget_with_bottom > 0 && hidden_below_with_bottom >= 2 {
        (start, end_with_bottom, has_top, true)
    } else {
        let end_no_bottom = end_for_budget(start, total, heights, slots_after_top);
        (start, end_no_bottom, has_top, false)
    }
}

pub(crate) fn calc_list_window_for_items(
    offset: usize,
    items: &[ListItem],
    max_display: usize,
) -> (usize, usize, bool, bool) {
    let heights: Vec<usize> = items.iter().map(list_item_height).collect();
    calc_list_window_with_heights(offset, items.len(), max_display, &heights)
}

pub(crate) fn calc_list_window_for_items_with_indicators(
    offset: usize,
    items: &[ListItem],
    max_display: usize,
    show_scroll_indicators: bool,
) -> (usize, usize, bool, bool) {
    if show_scroll_indicators {
        return calc_list_window_for_items(offset, items, max_display);
    }

    if max_display == 0 || items.is_empty() {
        return (0, 0, false, false);
    }

    let start = offset.min(items.len().saturating_sub(1));
    let visible = visible_items_for_height(items, start, max_display.min(u16::MAX as usize) as u16);
    let end = start.saturating_add(visible).min(items.len());
    (start, end, false, false)
}

pub(crate) fn clamp_bottom_glued_offset_for_items(
    offset: usize,
    items: &[ListItem],
    max_display: usize,
    show_scroll_indicators: bool,
) -> usize {
    if max_display == 0 || items.is_empty() {
        return 0;
    }

    let total = items.len();
    let mut start = offset.min(total.saturating_sub(1));

    loop {
        let (cur_start, cur_end, _, _) = calc_list_window_for_items_with_indicators(
            start,
            items,
            max_display,
            show_scroll_indicators,
        );
        start = cur_start;

        // Not currently bottom-anchored (or already at top): nothing to tighten.
        if cur_end < total || start == 0 {
            return start;
        }

        let prev_start = start.saturating_sub(1);
        let (_prev_start, prev_end, _, _) = calc_list_window_for_items_with_indicators(
            prev_start,
            items,
            max_display,
            show_scroll_indicators,
        );

        if prev_end == total {
            start = prev_start;
            continue;
        }

        return start;
    }
}

pub(crate) fn item_index_at_visual_line(
    items: &[ListItem],
    start: usize,
    rel_line: usize,
    visible_item_count: usize,
) -> Option<usize> {
    if visible_item_count == 0 {
        return None;
    }

    let end = start.saturating_add(visible_item_count).min(items.len());
    let mut y = 0usize;
    for (idx, item) in items.iter().enumerate().take(end).skip(start) {
        let h = list_item_height(item);
        if rel_line < y.saturating_add(h) {
            return Some(idx);
        }
        y = y.saturating_add(h);
    }

    None
}

pub(crate) fn calc_list_window(
    offset: usize,
    total: usize,
    max_display: usize,
) -> (usize, usize, bool, bool) {
    if total <= max_display {
        return (0, total, false, false);
    }

    // Clamp offset into range (we know total > 0 here).
    let start = offset.min(total.saturating_sub(1));

    // "1 more" indicators are forbidden because they replace a real item row.
    // We still allow a window that hides exactly 1 item; we just won't show an indicator for it.
    //
    // This means:
    // - top indicator is shown only if there are >=2 hidden above -> start >= 2
    // - bottom indicator is shown only if there are >=2 hidden below
    let has_top = start >= 2;
    let top_reserved = has_top as usize;

    // Decide bottom indicator visibility based on the *actual* hidden count when the
    // indicator takes a row.
    //
    // With bottom indicator enabled, item slots shrink by 1. If that would leave only
    // 1 hidden item, we suppress the indicator and show the item instead.
    let slots_with_bottom = max_display.saturating_sub(top_reserved).saturating_sub(1);
    let end_with_bottom = start.saturating_add(slots_with_bottom).min(total);
    let hidden_below_with_bottom = total.saturating_sub(end_with_bottom);

    if slots_with_bottom > 0 && hidden_below_with_bottom >= 2 {
        (start, end_with_bottom, has_top, true)
    } else {
        let slots_no_bottom = max_display.saturating_sub(top_reserved);
        let end_no_bottom = start.saturating_add(slots_no_bottom).min(total);
        (start, end_no_bottom, has_top, false)
    }
}

pub(crate) fn list_scrollbar_metrics(
    items: &[ListItem],
    offset: usize,
    track_height: usize,
    show_scroll_indicators: bool,
) -> Option<ScrollbarMetrics> {
    list_scrollbar_metrics_inner(items, offset, track_height, show_scroll_indicators, false)
}

pub(crate) fn list_scrollbar_metrics_half(
    items: &[ListItem],
    offset: usize,
    track_height: usize,
    show_scroll_indicators: bool,
) -> Option<ScrollbarMetrics> {
    list_scrollbar_metrics_inner(items, offset, track_height, show_scroll_indicators, true)
}

fn list_scrollbar_metrics_inner(
    items: &[ListItem],
    offset: usize,
    track_height: usize,
    show_scroll_indicators: bool,
    half_cell: bool,
) -> Option<ScrollbarMetrics> {
    let total = items.len();
    let max_display =
        max_visible_items_for_height(items, track_height.min(u16::MAX as usize) as u16);

    if total == 0 || max_display == 0 || track_height == 0 {
        return None;
    }

    let visible_for_scroll = if show_scroll_indicators && total > max_display {
        max_display.saturating_sub(1)
    } else {
        max_display
    };

    if visible_for_scroll == 0 || total <= visible_for_scroll {
        return None;
    }

    // In half-cell mode, compute thumb geometry in doubled units for sub-cell precision.
    let effective_track = if half_cell {
        track_height.saturating_mul(2)
    } else {
        track_height
    };

    let visible_for_thumb = max_display;
    let max_offset = clamp_bottom_glued_offset_for_items(
        total.saturating_sub(1),
        items,
        track_height,
        show_scroll_indicators,
    );
    let clamped_offset = offset.min(max_offset);

    let mut thumb_len = round_mul_div(effective_track, visible_for_thumb, total)
        .max(1)
        .min(effective_track);

    if total > visible_for_thumb && effective_track > 1 && thumb_len == effective_track {
        thumb_len = effective_track - 1;
    }

    let max_thumb_start = effective_track.saturating_sub(thumb_len);

    // Skip the "offset=1" slot (it would hide one item without a top indicator).
    let compressed_max_offset = if show_scroll_indicators && max_offset > 0 {
        max_offset.saturating_sub(1)
    } else {
        max_offset
    };
    let compressed_offset = if show_scroll_indicators && clamped_offset > 0 {
        clamped_offset.saturating_sub(1)
    } else {
        clamped_offset
    };

    let mut thumb_start = if compressed_max_offset == 0 {
        0
    } else {
        round_mul_div(max_thumb_start, compressed_offset, compressed_max_offset)
    };

    // Reserve the final thumb slot for the true bottom-glued state only.
    // This keeps the thumb from visually reaching the bottom while there are
    // still hidden items below the current window.
    if max_thumb_start > 0 && clamped_offset < max_offset && thumb_start == max_thumb_start {
        thumb_start = thumb_start.saturating_sub(1);
    }

    Some(ScrollbarMetrics {
        thumb_len,
        thumb_start,
        max_thumb_start,
        max_offset,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        calc_list_window, calc_list_window_for_items_with_indicators,
        clamp_bottom_glued_offset_for_items, list_scrollbar_metrics,
    };
    use crate::widgets::ListItem;

    #[test]
    fn calc_list_window_suppresses_1_more_indicators_but_keeps_offset() {
        let (start, end, top, bottom) = calc_list_window(1, 100, 10);
        assert_eq!((start, top), (1, false));
        assert!(end > start);
        assert!(bottom);

        let (start, _end, top, _bottom) = calc_list_window(2, 100, 10);
        assert_eq!((start, top), (2, true));

        let (start, end, top, bottom) = calc_list_window(10, 20, 10);
        assert_eq!((start, end, top, bottom), (10, 18, true, true));

        let (start, end, top, bottom) = calc_list_window(0, 6, 5);
        assert_eq!((start, end, top, bottom), (0, 4, false, true));

        let (start, end, top, bottom) = calc_list_window(5, 10, 6);
        assert_eq!((start, end, top, bottom), (5, 10, true, false));
    }

    #[test]
    fn calc_list_window_without_indicators_uses_full_available_height() {
        let items: Vec<ListItem> = (0..20)
            .map(|i| ListItem::new(format!("Item {i}")))
            .collect();

        let (start, end, top, bottom) =
            calc_list_window_for_items_with_indicators(0, &items, 6, false);
        assert_eq!((start, end, top, bottom), (0, 6, false, false));

        let (start, end, top, bottom) =
            calc_list_window_for_items_with_indicators(5, &items, 6, false);
        assert_eq!((start, end, top, bottom), (5, 11, false, false));
    }

    #[test]
    fn clamp_bottom_glued_offset_moves_up_when_viewport_grows() {
        let items: Vec<ListItem> = (0..10)
            .map(|i| ListItem::new(format!("Item {i}")))
            .collect();

        let new_start = clamp_bottom_glued_offset_for_items(6, &items, 6, true);
        assert_eq!(new_start, 5);

        let (_s, end, _t, _b) =
            calc_list_window_for_items_with_indicators(new_start, &items, 6, true);
        assert_eq!(end, items.len());
    }

    #[test]
    fn list_scrollbar_metrics_reaches_bottom_on_final_offset_with_indicators() {
        let items: Vec<ListItem> = (0..10)
            .map(|i| ListItem::new(format!("Item {i}")))
            .collect();

        let metrics = list_scrollbar_metrics(&items, 5, 6, true).expect("scrollbar metrics");

        assert_eq!(metrics.max_offset, 5);
        assert_eq!(metrics.max_thumb_start, 2);
        assert_eq!(metrics.thumb_start, 2);
    }

    #[test]
    fn list_scrollbar_metrics_keeps_bottom_position_on_final_step_with_indicators() {
        let items: Vec<ListItem> = (0..10)
            .map(|i| ListItem::new(format!("Item {i}")))
            .collect();

        let penultimate = list_scrollbar_metrics(&items, 4, 6, true).expect("penultimate metrics");
        let final_metrics = list_scrollbar_metrics(&items, 5, 6, true).expect("final metrics");

        assert_eq!(penultimate.thumb_start, 1);
        assert!(penultimate.thumb_start < penultimate.max_thumb_start);
        assert_eq!(final_metrics.thumb_start, final_metrics.max_thumb_start);
        assert!(final_metrics.thumb_start > penultimate.thumb_start);
    }

    #[test]
    fn list_scrollbar_metrics_keeps_thumb_len_stable_at_bottom_with_indicators() {
        let items: Vec<ListItem> = (0..9).map(|i| ListItem::new(format!("Item {i}"))).collect();

        let top = list_scrollbar_metrics(&items, 0, 5, true).expect("top metrics");
        let near_bottom = list_scrollbar_metrics(&items, 4, 5, true).expect("near-bottom metrics");
        let bottom = list_scrollbar_metrics(&items, 5, 5, true).expect("bottom metrics");

        assert_eq!(top.thumb_len, 3);
        assert_eq!(near_bottom.thumb_len, 3);
        assert_eq!(bottom.thumb_len, 3);
    }

    #[test]
    fn list_scrollbar_metrics_does_not_use_bottom_slot_while_items_remain_below() {
        let items: Vec<ListItem> = (0..9).map(|i| ListItem::new(format!("Item {i}"))).collect();

        let (_start, end, _has_top, has_bottom) =
            calc_list_window_for_items_with_indicators(4, &items, 5, true);
        let metrics = list_scrollbar_metrics(&items, 4, 5, true).expect("near-bottom metrics");

        assert_eq!(end, 7);
        assert!(has_bottom);
        assert_eq!(metrics.max_thumb_start, 2);
        assert_eq!(metrics.thumb_start, 1);
        assert!(metrics.thumb_start < metrics.max_thumb_start);
    }

    #[test]
    fn list_scrollbar_metrics_uses_bottom_slot_only_for_true_bottom_window() {
        let items: Vec<ListItem> = (0..9).map(|i| ListItem::new(format!("Item {i}"))).collect();

        for offset in 0..=5 {
            let (_start, end, _has_top, has_bottom) =
                calc_list_window_for_items_with_indicators(offset, &items, 5, true);
            let metrics = list_scrollbar_metrics(&items, offset, 5, true).expect("metrics");

            if end < items.len() {
                assert!(has_bottom, "offset={offset} should still have items below");
                assert!(
                    metrics.thumb_start < metrics.max_thumb_start,
                    "offset={offset} should not use the final thumb slot before true bottom"
                );
            } else {
                assert!(
                    !has_bottom,
                    "offset={offset} should be the true bottom window"
                );
                assert_eq!(metrics.thumb_start, metrics.max_thumb_start);
            }
        }
    }

    #[test]
    fn list_scrollbar_metrics_without_indicators_does_not_use_bottom_slot_before_true_bottom() {
        let items: Vec<ListItem> = (0..10)
            .map(|i| ListItem::new(format!("Item {i}")))
            .collect();

        let (start, end, has_top, has_bottom) =
            calc_list_window_for_items_with_indicators(3, &items, 6, false);
        let metrics = list_scrollbar_metrics(&items, 3, 6, false).expect("near-bottom metrics");

        assert_eq!((start, end, has_top, has_bottom), (3, 9, false, false));
        assert_eq!(metrics.max_offset, 4);
        assert_eq!(metrics.max_thumb_start, 2);
        assert_eq!(metrics.thumb_start, 1);
        assert!(metrics.thumb_start < metrics.max_thumb_start);
    }

    #[test]
    fn list_scrollbar_metrics_without_indicators_uses_bottom_slot_only_for_true_bottom() {
        let items: Vec<ListItem> = (0..10)
            .map(|i| ListItem::new(format!("Item {i}")))
            .collect();

        for offset in 0..=4 {
            let (_start, end, _has_top, _has_bottom) =
                calc_list_window_for_items_with_indicators(offset, &items, 6, false);
            let metrics = list_scrollbar_metrics(&items, offset, 6, false).expect("metrics");

            if end < items.len() {
                assert!(
                    metrics.thumb_start < metrics.max_thumb_start,
                    "offset={offset} should not use the final thumb slot before true bottom"
                );
            } else {
                assert_eq!(
                    metrics.thumb_start, metrics.max_thumb_start,
                    "offset={offset} should use the final thumb slot only at true bottom"
                );
            }
        }
    }
}
