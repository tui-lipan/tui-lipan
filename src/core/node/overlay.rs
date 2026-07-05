use std::sync::Arc;

use super::id::NodeId;
use crate::callback::Callback;
use crate::overlay::{DismissPolicy, OverlayId, OverlayLayer, PointerCapture};
use crate::style::{Rect, ScrollbarVariant, Style};

#[derive(Clone)]
pub(crate) struct OverlayRoot {
    pub(crate) id: NodeId,
    pub(crate) overlay_id: Option<OverlayId>,
    pub(crate) layer: OverlayLayer,
    pub(crate) order: u64,
    pub(crate) dismiss_policy: DismissPolicy,
    pub(crate) on_dismiss: Option<Callback<()>>,
    pub(crate) backdrop: Option<Style>,
    pub(crate) opacity: f32,
    pub(crate) captures_focus: bool,
    pub(crate) captures_pointer: PointerCapture,
    pub(crate) copy_text: Option<Arc<str>>,
    pub(crate) copy_zone: Option<Rect>,
    pub(crate) copy_feedback_active: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScrollbarAxis {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollbarZone {
    pub id: NodeId,
    pub axis: ScrollbarAxis,
    pub rect: Rect,
}

impl ScrollbarZone {
    pub(crate) fn contains(&self, x: i16, y: i16) -> bool {
        self.rect.contains(x, y)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollbarTarget {
    pub id: NodeId,
    pub axis: ScrollbarAxis,
}

/// Compute the `ScrollbarZone` for a vertical scrollbar track.
///
/// This encapsulates the geometry that is shared across List, Table, TextArea,
/// Terminal, ScrollView and DocumentView.
///
/// * `inner_h` - effective inner height; callers that reserve a row for a
///   horizontal scrollbar should pass the reduced value.
pub(crate) fn vertical_scrollbar_zone(
    id: NodeId,
    rect: Rect,
    inner: Rect,
    inner_h: u16,
    border: bool,
    scrollbar_variant: ScrollbarVariant,
    parent_border_x: Option<i16>,
) -> ScrollbarZone {
    let use_integrated = matches!(scrollbar_variant, ScrollbarVariant::Integrated)
        && (border || parent_border_x.is_some());
    let x = if use_integrated {
        if border {
            rect.x.saturating_add(rect.w.saturating_sub(1) as i16)
        } else {
            parent_border_x
                .unwrap_or_else(|| rect.x.saturating_add(rect.w.saturating_sub(1) as i16))
        }
    } else {
        inner.x.saturating_add(inner.w.saturating_sub(1) as i16)
    };

    ScrollbarZone {
        id,
        axis: ScrollbarAxis::Vertical,
        rect: Rect {
            x,
            y: inner.y,
            w: 1,
            h: inner_h,
        },
    }
}

/// Geometry inputs for [`compute_scrollbar_zones`].
pub(crate) struct ScrollbarZonesParams {
    pub id: NodeId,
    pub rect: Rect,
    pub inner: Rect,
    pub border: bool,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub h_scrollbar: bool,
    pub h_scrollbar_variant: ScrollbarVariant,
    pub content_x: i16,
    pub content_width: u16,
    pub max_content_width: usize,
    pub wrap: bool,
    pub parent_border_x: Option<i16>,
    pub parent_border_y: Option<i16>,
}

/// Compute both vertical and horizontal scrollbar zones for widgets with dual
/// scrollbars (DocumentView, TextArea).
///
/// * `content_x` - x origin of the content area (after gutter).
/// * `content_width` - width of the content area before scrollbar-column
///   deduction; the helper subtracts scrollbar columns itself.
/// * `max_content_width` - maximum content width in columns, used to decide
///   whether the horizontal scrollbar is visible.
pub(crate) fn compute_scrollbar_zones(params: ScrollbarZonesParams) -> Vec<ScrollbarZone> {
    let ScrollbarZonesParams {
        id,
        rect,
        inner,
        border,
        scrollbar,
        scrollbar_variant,
        scrollbar_gap,
        h_scrollbar,
        h_scrollbar_variant,
        content_x,
        content_width,
        max_content_width,
        wrap,
        parent_border_x,
        parent_border_y,
    } = params;
    let mut zones = Vec::new();

    let v_scrollbar_over_border = scrollbar
        && matches!(scrollbar_variant, ScrollbarVariant::Integrated)
        && (border || parent_border_x.is_some());
    let scrollbar_cols: u16 = if scrollbar && !v_scrollbar_over_border {
        1u16.saturating_add(scrollbar_gap)
    } else {
        0
    };

    let effective_content_width = content_width.saturating_sub(scrollbar_cols);
    let h_scrollbar_visible =
        h_scrollbar && !wrap && max_content_width > effective_content_width as usize;
    let h_scrollbar_over_border = h_scrollbar
        && matches!(h_scrollbar_variant, ScrollbarVariant::Integrated)
        && (border || parent_border_y.is_some());

    let mut inner_h = inner.h;
    if h_scrollbar_visible && !h_scrollbar_over_border {
        inner_h = inner_h.saturating_sub(1);
    }

    if scrollbar {
        zones.push(vertical_scrollbar_zone(
            id,
            rect,
            inner,
            inner_h,
            border,
            scrollbar_variant,
            parent_border_x,
        ));
    }

    if h_scrollbar_visible {
        let use_integrated = matches!(h_scrollbar_variant, ScrollbarVariant::Integrated)
            && (border || parent_border_y.is_some());
        let y = if use_integrated {
            if border {
                rect.y.saturating_add(rect.h.saturating_sub(1) as i16)
            } else {
                parent_border_y
                    .unwrap_or_else(|| rect.y.saturating_add(rect.h.saturating_sub(1) as i16))
            }
        } else {
            inner.y.saturating_add(inner_h as i16)
        };

        zones.push(ScrollbarZone {
            id,
            axis: ScrollbarAxis::Horizontal,
            rect: Rect {
                x: content_x,
                y,
                w: effective_content_width,
                h: 1,
            },
        });
    }

    zones
}
