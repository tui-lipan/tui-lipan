use crate::widgets::list::utils::calc_list_window;

pub(crate) fn smart_list_offset_with_indicators(
    old_offset: usize,
    selected: usize,
    total: usize,
    max_display: usize,
) -> usize {
    if max_display == 0 || total == 0 {
        return 0;
    }

    if total <= max_display {
        return 0;
    }

    let input_offset = old_offset;
    let mut offset = old_offset;

    let normalize_offset_one = |off: usize| -> usize {
        if off != 1 {
            return off;
        }

        let preferred = if off < input_offset {
            0
        } else if selected >= 2 {
            2
        } else {
            0
        }
        .min(total.saturating_sub(1));

        let (s, e, _, _) = calc_list_window(preferred, total, max_display);
        if selected >= s && selected < e {
            preferred
        } else {
            off
        }
    };

    offset = normalize_offset_one(offset);

    let (start, end, _has_top, has_bottom) = calc_list_window(offset, total, max_display);
    let effective_lines = end.saturating_sub(start);
    let bottom_reserved = has_bottom as usize;

    let buffer = if max_display <= 6 { 1 } else { 2 };

    if effective_lines.saturating_add(bottom_reserved) > buffer {
        let item_line = selected.saturating_sub(start);
        let max_item_line = effective_lines
            .saturating_sub(1)
            .saturating_add(bottom_reserved)
            .saturating_sub(buffer);
        if item_line > max_item_line {
            let delta = item_line - max_item_line;
            offset = offset.saturating_add(delta);
        }
    }

    if selected < start {
        offset = selected;
    }

    offset = normalize_offset_one(offset);

    let (start, _, has_top, _) = calc_list_window(offset, total, max_display);
    let top_reserved = has_top as usize;

    {
        let item_line = selected.saturating_sub(start);
        let min_item_line = buffer.saturating_sub(top_reserved);
        if item_line < min_item_line {
            let delta = min_item_line.saturating_sub(item_line);
            offset = offset.saturating_sub(delta.min(offset));
        }
    }

    offset = normalize_offset_one(offset);

    let (start, end, has_top, _) = calc_list_window(offset, total, max_display);
    if end >= total {
        let mut slots = max_display;
        if has_top {
            slots = slots.saturating_sub(1);
        }
        let potential_end = start.saturating_add(slots);
        if potential_end > total {
            let diff = potential_end.saturating_sub(total);
            offset = offset.saturating_sub(diff);
        }
    }

    offset = normalize_offset_one(offset);

    let offset = ensure_selected_visible_with_indicators(offset, selected, total, max_display);
    let (final_start, final_end, _, _) = calc_list_window(offset, total, max_display);

    if selected < final_start || selected >= final_end {
        let fallback = selected.min(total.saturating_sub(1));
        let (fallback_start, _, _, _) = calc_list_window(fallback, total, max_display);
        return fallback_start;
    }

    offset
}

fn ensure_selected_visible_with_indicators(
    mut offset: usize,
    selected: usize,
    total: usize,
    max_display: usize,
) -> usize {
    if total == 0 || max_display == 0 {
        return 0;
    }

    let mut guard = 0usize;
    let max_guard = total.saturating_mul(2).max(1);

    while guard < max_guard {
        let (start, end, _, _) = calc_list_window(offset, total, max_display);

        if end <= start {
            return selected.min(total.saturating_sub(1));
        }

        if selected < start {
            let delta = start.saturating_sub(selected).max(1);
            let next = offset.saturating_sub(delta);
            if next == offset {
                return offset;
            }
            offset = next;
            guard = guard.saturating_add(1);
            continue;
        }

        if selected >= end {
            let last_visible = end.saturating_sub(1);
            let delta = selected.saturating_sub(last_visible).max(1);
            let next = offset.saturating_add(delta).min(total.saturating_sub(1));
            if next == offset {
                return offset;
            }
            offset = next;
            guard = guard.saturating_add(1);
            continue;
        }

        return offset;
    }

    offset
}

pub(crate) fn smart_list_offset(
    old_offset: usize,
    selected: usize,
    len: usize,
    height: u16,
) -> usize {
    let height = height as usize;
    if height == 0 || len == 0 {
        return 0;
    }

    let margin = if height <= 2 {
        0
    } else if height <= 6 {
        1
    } else {
        2
    };

    let max_possible_offset = len.saturating_sub(height);

    let max_allowed = selected.saturating_sub(margin);
    let min_allowed = selected.saturating_sub(height.saturating_sub(1).saturating_sub(margin));

    let mut new_offset = old_offset.max(min_allowed).min(max_allowed);

    new_offset = new_offset.min(max_possible_offset);

    new_offset
}

#[cfg(test)]
mod tests {
    use super::{smart_list_offset, smart_list_offset_with_indicators};
    use crate::widgets::list::utils::calc_list_window;

    #[test]
    fn list_offset_resize_behavior() {
        let offset = smart_list_offset(0, 50, 100, 10);
        assert_eq!(offset, 43);

        let offset = smart_list_offset(offset, 50, 100, 5);
        assert_eq!(offset, 47);

        let offset = smart_list_offset(offset, 50, 100, 20);
        assert_eq!(offset, 47);
    }

    #[test]
    fn smart_list_offset_respects_margins() {
        assert_eq!(smart_list_offset(0, 7, 100, 10), 0);
        assert_eq!(smart_list_offset(0, 8, 100, 10), 1);
        assert_eq!(smart_list_offset(1, 9, 100, 10), 2);
        assert_eq!(smart_list_offset(10, 12, 100, 10), 10);
        assert_eq!(smart_list_offset(10, 11, 100, 10), 9);
    }

    #[test]
    fn smart_list_offset_adaptive_margins() {
        assert_eq!(smart_list_offset(0, 3, 100, 5), 0);
        assert_eq!(smart_list_offset(0, 4, 100, 5), 1);
        assert_eq!(smart_list_offset(0, 1, 100, 2), 0);
    }

    #[test]
    fn smart_list_offset_with_indicators_counts_indicator_rows_in_buffer() {
        assert_eq!(smart_list_offset_with_indicators(0, 3, 100, 5), 0);
        assert_eq!(smart_list_offset_with_indicators(0, 4, 100, 5), 2);
        assert_eq!(smart_list_offset_with_indicators(10, 10, 100, 5), 10);
    }

    #[test]
    fn smart_list_offset_with_indicators_keeps_selection_visible_near_top() {
        let new_offset = smart_list_offset_with_indicators(0, 1, 3, 2);
        let (start, end, _, _) = calc_list_window(new_offset, 3, 2);
        assert!(
            start <= 1 && 1 < end,
            "selected row hidden: offset={new_offset}, window={start}..{end}"
        );
    }

    #[test]
    fn smart_list_offset_with_indicators_keeps_selection_visible_for_small_windows() {
        for total in 1..20 {
            for max_display in 2..8 {
                for old_offset in 0..total {
                    for selected in 0..total {
                        let new_offset = smart_list_offset_with_indicators(
                            old_offset,
                            selected,
                            total,
                            max_display,
                        );
                        let (start, end, _, _) = calc_list_window(new_offset, total, max_display);
                        assert!(
                            start <= selected && selected < end,
                            "hidden selection: total={total}, max_display={max_display}, old={old_offset}, selected={selected}, new={new_offset}, window={start}..{end}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn smart_list_offset_with_indicators_group_boundary_is_stable() {
        let first = smart_list_offset_with_indicators(0, 6, 9, 6);
        assert_eq!(first, 3);

        let second = smart_list_offset_with_indicators(first, 6, 9, 6);
        assert_eq!(second, first);
    }

    #[test]
    fn smart_list_offset_brings_scrolled_away_selection_back_into_view() {
        let new_offset = smart_list_offset(12, 3, 20, 10);
        assert!(
            new_offset <= 3 && new_offset + 10 > 3,
            "item 3 not visible: offset={new_offset}"
        );

        let new_offset = smart_list_offset(0, 15, 20, 10);
        assert!(
            new_offset <= 15 && new_offset + 10 > 15,
            "item 15 not visible: offset={new_offset}"
        );
    }

    #[test]
    fn smart_list_offset_with_indicators_brings_scrolled_away_selection_back_into_view() {
        let new_offset = smart_list_offset_with_indicators(12, 3, 20, 10);
        let (start, end, _, _) = calc_list_window(new_offset, 20, 10);
        assert!(
            start <= 3 && 3 < end,
            "item 3 not visible after scrolling down: window={start}..{end}"
        );

        let new_offset = smart_list_offset_with_indicators(0, 15, 20, 10);
        let (start, end, _, _) = calc_list_window(new_offset, 20, 10);
        assert!(
            start <= 15 && 15 < end,
            "item 15 not visible after scrolling up: window={start}..{end}"
        );
    }

    #[test]
    fn smart_list_offset_with_indicators_skips_offset_one_when_possible() {
        assert_eq!(smart_list_offset_with_indicators(1, 2, 100, 10), 2);
        assert_eq!(smart_list_offset_with_indicators(0, 8, 100, 10), 2);
        assert_eq!(smart_list_offset_with_indicators(1, 1, 100, 10), 0);
    }
}
