use crate::utils::math::round_mul_div;
use crate::utils::scrollbar::ScrollbarMetrics;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScrollViewWindow {
    pub offset: usize,
    pub max_offset: usize,
    pub visible_rows: usize,
    pub top_indicator: bool,
    pub bottom_indicator: bool,
    pub bottom_count: usize,
}

pub(crate) fn calc_scroll_view_window(
    offset: usize,
    total_rows: usize,
    viewport_rows: usize,
    show_scroll_indicators: bool,
) -> ScrollViewWindow {
    if total_rows == 0 || viewport_rows == 0 {
        return ScrollViewWindow {
            offset: 0,
            max_offset: 0,
            visible_rows: 0,
            top_indicator: false,
            bottom_indicator: false,
            bottom_count: 0,
        };
    }

    if !show_scroll_indicators || total_rows <= viewport_rows {
        let max_offset = total_rows.saturating_sub(viewport_rows);
        let offset = offset.min(max_offset);
        return ScrollViewWindow {
            offset,
            max_offset,
            visible_rows: total_rows.saturating_sub(offset).min(viewport_rows),
            top_indicator: false,
            bottom_indicator: false,
            bottom_count: 0,
        };
    }

    let max_offset = total_rows.saturating_sub(viewport_rows).saturating_add(1);
    let offset = offset.min(max_offset);

    let top_indicator = offset >= 2;
    let top_reserved = top_indicator as usize;

    let slots_with_bottom = viewport_rows.saturating_sub(top_reserved).saturating_sub(1);
    let end_with_bottom = offset.saturating_add(slots_with_bottom).min(total_rows);
    let hidden_below_with_bottom = total_rows.saturating_sub(end_with_bottom);

    if slots_with_bottom > 0 && hidden_below_with_bottom >= 2 {
        ScrollViewWindow {
            offset,
            max_offset,
            visible_rows: slots_with_bottom,
            top_indicator,
            bottom_indicator: true,
            bottom_count: hidden_below_with_bottom,
        }
    } else {
        let visible_rows = viewport_rows.saturating_sub(top_reserved);
        ScrollViewWindow {
            offset,
            max_offset,
            visible_rows: total_rows.saturating_sub(offset).min(visible_rows),
            top_indicator,
            bottom_indicator: false,
            bottom_count: 0,
        }
    }
}

pub(crate) fn normalize_input_offset(
    previous_offset: usize,
    target_offset: usize,
    total_rows: usize,
    viewport_rows: usize,
    show_scroll_indicators: bool,
) -> usize {
    let window = calc_scroll_view_window(
        target_offset,
        total_rows,
        viewport_rows,
        show_scroll_indicators,
    );
    let offset = window.offset;

    if show_scroll_indicators && offset == 1 {
        if previous_offset > offset {
            0
        } else {
            2.min(window.max_offset)
        }
    } else {
        offset
    }
}

pub(crate) fn scroll_view_scrollbar_metrics(
    offset: usize,
    total_rows: usize,
    viewport_rows: usize,
    track_size: usize,
    show_scroll_indicators: bool,
    half_cell: bool,
) -> Option<ScrollbarMetrics> {
    if total_rows == 0 || viewport_rows == 0 || track_size == 0 || total_rows <= viewport_rows {
        return None;
    }

    let effective_track = if half_cell {
        track_size.saturating_mul(2)
    } else {
        track_size
    };
    let window = calc_scroll_view_window(offset, total_rows, viewport_rows, show_scroll_indicators);
    let clamped_offset = window.offset.min(window.max_offset);

    let mut thumb_len = round_mul_div(effective_track, viewport_rows, total_rows)
        .max(1)
        .min(effective_track);
    if total_rows > viewport_rows && effective_track > 1 && thumb_len == effective_track {
        thumb_len = effective_track - 1;
    }

    let max_thumb_start = effective_track.saturating_sub(thumb_len);
    let compressed_max_offset = if show_scroll_indicators && window.max_offset > 0 {
        window.max_offset.saturating_sub(1)
    } else {
        window.max_offset
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
    if max_thumb_start > 0 && clamped_offset < window.max_offset && thumb_start == max_thumb_start {
        thumb_start = thumb_start.saturating_sub(1);
    }

    Some(ScrollbarMetrics {
        thumb_len,
        thumb_start,
        max_thumb_start,
        max_offset: window.max_offset,
    })
}

#[cfg(test)]
mod tests {
    use super::{calc_scroll_view_window, normalize_input_offset, scroll_view_scrollbar_metrics};

    #[test]
    fn suppresses_one_above_and_one_below_indicators() {
        let window = calc_scroll_view_window(1, 100, 10, true);
        assert_eq!(window.offset, 1);
        assert!(!window.top_indicator);
        assert!(window.bottom_indicator);

        let window = calc_scroll_view_window(5, 10, 6, true);
        assert_eq!(window.offset, 5);
        assert!(window.top_indicator);
        assert!(!window.bottom_indicator);
    }

    #[test]
    fn keeps_bottom_slot_for_true_bottom_only() {
        let before_bottom = scroll_view_scrollbar_metrics(5, 20, 10, 10, true, false)
            .expect("metrics before bottom");
        let at_bottom =
            scroll_view_scrollbar_metrics(11, 20, 10, 10, true, false).expect("metrics at bottom");

        assert!(before_bottom.thumb_start < before_bottom.max_thumb_start);
        assert_eq!(at_bottom.thumb_start, at_bottom.max_thumb_start);
    }

    #[test]
    fn half_cell_metrics_share_compressed_indicator_mapping() {
        let top = scroll_view_scrollbar_metrics(0, 20, 10, 10, true, true).expect("top metrics");
        let one_hidden =
            scroll_view_scrollbar_metrics(1, 20, 10, 10, true, true).expect("one hidden");
        let two_hidden =
            scroll_view_scrollbar_metrics(2, 20, 10, 10, true, true).expect("two hidden");

        assert_eq!(top.thumb_start, one_hidden.thumb_start);
        assert!(two_hidden.thumb_start > top.thumb_start);
    }

    #[test]
    fn input_normalization_skips_hidden_one_above_when_scrolling_up() {
        assert_eq!(normalize_input_offset(4, 1, 12, 8, true), 0);
    }

    #[test]
    fn input_normalization_skips_hidden_one_above_when_scrolling_down() {
        assert_eq!(normalize_input_offset(0, 1, 12, 8, true), 2);
    }
}
