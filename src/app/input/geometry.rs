//! Shared geometry helpers for slider and progress bar hit-testing.

use unicode_width::UnicodeWidthStr;

use crate::style::Rect;
use crate::widgets::ProgressTextPosition;
use crate::widgets::internal::{ProgressNode, SliderNode};

/// Computed slider track geometry after accounting for label and value display.
pub(crate) struct SliderTrack {
    /// Left edge of the track (column).
    pub track_x: i16,
    /// Width of the track in cells.
    pub track_w: u16,
    /// Y position of the track (vertical center of the inner rect).
    pub track_y: i16,
}

/// Compute the slider track geometry from a `SliderNode` and its layout rect.
///
/// Returns `None` if the track would have zero width or height.
pub(crate) fn slider_track_geometry(slider: &SliderNode, rect: Rect) -> Option<SliderTrack> {
    let inner = rect.inset(slider.padding);
    if inner.w == 0 || inner.h == 0 {
        return None;
    }

    let mut track_x = inner.x;
    let mut track_w = inner.w;

    if let Some(l) = &slider.label {
        let label_w = UnicodeWidthStr::width(l.as_str() as &str) as u16;
        if track_w > label_w {
            track_x = track_x.saturating_add(label_w as i16 + 1);
            track_w = track_w.saturating_sub(label_w + 1);
        }
    }

    if slider.show_value {
        let value_w = crate::widgets::slider::value_slot_width(slider.min, slider.max);
        if track_w > value_w {
            track_w = track_w.saturating_sub(value_w + 1);
        }
    }

    if track_w == 0 {
        return None;
    }

    let track_y = inner
        .y
        .saturating_add((inner.h.saturating_sub(1) / 2) as i16);

    Some(SliderTrack {
        track_x,
        track_w,
        track_y,
    })
}

/// Compute the progress bar value from a mouse x-coordinate.
///
/// Returns `None` if the progress bar has zero width after padding.
pub(crate) fn progress_value_at_x(progress: &ProgressNode, rect: Rect, x: u16) -> Option<f64> {
    let inner = rect.inset(progress.padding);
    if inner.w == 0 {
        return None;
    }

    // Only Left/Right text positions consume horizontal track space.
    // Middle/Above/Below are rendered inside or outside the track row and
    // do not affect the bar width or track start offset.
    let pct_side = progress.show_percentage
        && matches!(
            progress.percentage_position,
            ProgressTextPosition::Left | ProgressTextPosition::Right
        );
    // Percentage is always rendered as 4 chars (" 50%", "100%"), plus 1 space separator.
    let pct_reserved: usize = if pct_side { 5 } else { 0 };
    let label_side = progress.label.is_some()
        && matches!(
            progress.label_position,
            ProgressTextPosition::Left | ProgressTextPosition::Right
        );
    let label_reserved: usize = if label_side {
        progress
            .label
            .as_ref()
            .map(|l| 1 + UnicodeWidthStr::width(l.as_str()))
            .unwrap_or(0)
    } else {
        0
    };

    let bar_w = inner
        .w
        .saturating_sub((pct_reserved + label_reserved) as u16);

    // For Left position the percentage is rendered before the track, so
    // the track start is shifted right by the percentage slot width.
    let mut track_offset = 0_i32;
    if pct_side && matches!(progress.percentage_position, ProgressTextPosition::Left) {
        track_offset += pct_reserved as i32;
    }
    if label_side && matches!(progress.label_position, ProgressTextPosition::Left) {
        track_offset += label_reserved as i32;
    }

    let rel_x = (x as i32)
        .saturating_sub(inner.x as i32)
        .saturating_sub(track_offset);

    let mut p_value: f64 = if bar_w <= 1 {
        if rel_x > 0 { 1.0 } else { 0.0 }
    } else if rel_x >= bar_w as i32 {
        1.0
    } else {
        (rel_x as f64 / bar_w as f64).clamp(0.0, 1.0)
    };

    if progress.inverted {
        p_value = 1.0 - p_value;
    }

    if let Some(step_size) = progress.step {
        let steps = (p_value / step_size).round();
        p_value = (steps * step_size).clamp(0.0, 1.0);
    }

    Some(p_value)
}
