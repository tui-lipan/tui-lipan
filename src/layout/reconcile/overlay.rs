use super::element::{ElementReconcile, reconcile_element};
use super::state::{OverlayState, ReconcileCtx};
use crate::core::component::FocusContext;
use crate::core::element::Element;
use crate::core::node::{NodeId, NodeKind, NodeTree, OverlayRoot};
use crate::layout::axis::{Axis, requested_main_axis};
use crate::layout::measure::min_size_constrained;
use crate::overlay::{
    DismissPolicy, OverlayEntry, OverlayLayer, OverlayPlacement, Portal, ToastPlacement,
};
use crate::style::{LayoutConstraints, Length, Padding, Rect};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

fn resolved_overlay_measure_max(
    content: &Element,
    overlay_constraints: Option<&LayoutConstraints>,
    bounds: Rect,
) -> (Option<u16>, Option<u16>) {
    let content_constraints = content.layout_constraints();
    let resolve_cap =
        |content_max: Option<Length>, overlay_max: Option<Length>, available: u16| match (
            content_max.and_then(|l| l.resolve_as_max(available)),
            overlay_max.and_then(|l| l.resolve_as_max(available)),
        ) {
            (Some(content), Some(overlay)) => Some(content.min(overlay)),
            (Some(content), None) => Some(content),
            (None, Some(overlay)) => Some(overlay),
            (None, None) => None,
        };

    (
        resolve_cap(
            content_constraints.max_w,
            overlay_constraints.and_then(|c| c.max_w),
            bounds.w,
        ),
        resolve_cap(
            content_constraints.max_h,
            overlay_constraints.and_then(|c| c.max_h),
            bounds.h,
        ),
    )
}

fn resolve_overlay_size(
    content: &Element,
    bounds: Rect,
    overlay_constraints: Option<&LayoutConstraints>,
) -> (u16, u16) {
    // In bounded overlay contexts (e.g. modal max_height), measure against the
    // effective max bounds so Auto children size within the real available area
    // instead of growing unconstrained and being clipped afterward.
    let (measure_max_w, measure_max_h) =
        resolved_overlay_measure_max(content, overlay_constraints, bounds);
    let (measured_w, measured_h) = min_size_constrained(content, measure_max_w, measure_max_h);
    let requested_w = requested_main_axis(content, Axis::Horizontal, None);
    let requested_h = requested_main_axis(content, Axis::Vertical, None);

    let mut width = requested_w.resolve(bounds.w, measured_w);
    let mut height = requested_h.resolve(bounds.h, measured_h);

    let constraints = content.layout_constraints();
    width = constraints.clamp_width(width, bounds.w).min(bounds.w);
    height = constraints.clamp_height(height, bounds.h).min(bounds.h);
    if let Some(overlay_constraints) = overlay_constraints {
        width = overlay_constraints
            .clamp_width(width, bounds.w)
            .min(bounds.w);
        height = overlay_constraints
            .clamp_height(height, bounds.h)
            .min(bounds.h);
    }

    (width, height)
}

pub(crate) fn resolve_center_rect(
    content: &Element,
    bounds: Rect,
    overlay_constraints: Option<&LayoutConstraints>,
) -> Rect {
    let (width, height) = resolve_overlay_size(content, bounds, overlay_constraints);
    let x = bounds
        .x
        .saturating_add((bounds.w.saturating_sub(width) / 2) as i16);
    let y = bounds
        .y
        .saturating_add((bounds.h.saturating_sub(height) / 2) as i16);
    Rect {
        x,
        y,
        w: width,
        h: height,
    }
}

pub(crate) fn resolve_stacked_rect(
    content: &Element,
    bounds: Rect,
    placement: ToastPlacement,
    margin: Padding,
    offset: u16,
    overlay_constraints: Option<&LayoutConstraints>,
) -> Rect {
    let (width, height) = resolve_overlay_size(content, bounds, overlay_constraints);

    let base_x = match placement {
        ToastPlacement::TopStart | ToastPlacement::BottomStart => {
            bounds.x.saturating_add(margin.left as i16)
        }
        ToastPlacement::TopCenter | ToastPlacement::BottomCenter => bounds
            .x
            .saturating_add((bounds.w.saturating_sub(width) / 2) as i16),
        ToastPlacement::TopEnd | ToastPlacement::BottomEnd => bounds
            .x
            .saturating_add(bounds.w.saturating_sub(width).saturating_sub(margin.right) as i16),
    };

    let base_y = match placement {
        ToastPlacement::TopStart | ToastPlacement::TopCenter | ToastPlacement::TopEnd => bounds
            .y
            .saturating_add(margin.top.saturating_add(offset) as i16),
        ToastPlacement::BottomStart | ToastPlacement::BottomCenter | ToastPlacement::BottomEnd => {
            bounds.y.saturating_add(
                bounds
                    .h
                    .saturating_sub(height)
                    .saturating_sub(margin.bottom.saturating_add(offset)) as i16,
            )
        }
    };

    Rect {
        x: base_x,
        y: base_y,
        w: width,
        h: height,
    }
}

pub(crate) fn reconcile_portal(
    tree: &mut NodeTree,
    epoch: u32,
    id: NodeId,
    portal: &Portal,
    constraints: &LayoutConstraints,
    focus: Option<&FocusContext>,
    overlay_state: &mut OverlayState,
) -> NodeId {
    let old_children = {
        let node = tree.node_mut(id);
        node.rect = Rect::default();
        node.kind = NodeKind::Portal(crate::overlay::PortalNode {
            content: Box::new(NodeId::INVALID),
        });
        std::mem::take(&mut node.children)
    };

    let reuse_child = old_children.first().copied();
    let reserve_max_height = matches!(
        portal.placement,
        OverlayPlacement::Center {
            reserve_max_height: true
        }
    );
    let mut content_rect = match &portal.placement {
        OverlayPlacement::Center { .. } => resolve_center_rect(
            portal.content.as_ref(),
            overlay_state.bounds,
            Some(constraints),
        ),
        OverlayPlacement::Stacked {
            placement, margin, ..
        } => resolve_stacked_rect(
            portal.content.as_ref(),
            overlay_state.bounds,
            *placement,
            *margin,
            0,
            Some(constraints),
        ),
    };
    let bounds = overlay_state.bounds;
    content_rect.w = constraints.clamp_width(content_rect.w, bounds.w);
    content_rect.h = constraints.clamp_height(content_rect.h, bounds.h);
    // Re-center after clamping so the modal stays centered when capped.
    content_rect.x = bounds
        .x
        .saturating_add((bounds.w.saturating_sub(content_rect.w) / 2) as i16);
    // When `reserve_max_height` is set, center as if the content occupied its full
    // `max_height` cap, then top-align the (possibly shorter) content within that reserved
    // band. This keeps the top edge fixed as content shrinks below the cap, instead of the
    // whole overlay drifting toward the vertical center.
    let reserved_h = reserve_max_height
        .then(|| constraints.max_h.and_then(|l| l.resolve_as_max(bounds.h)))
        .flatten()
        .map(|cap| cap.min(bounds.h).max(content_rect.h))
        .unwrap_or(content_rect.h);
    content_rect.y = bounds
        .y
        .saturating_add((bounds.h.saturating_sub(reserved_h) / 2) as i16);

    let content_id = reconcile_element(
        &mut ReconcileCtx {
            tree,
            epoch,
            focus,
            overlay_state,
        },
        ElementReconcile {
            reuse: reuse_child,
            parent: Some(id),
            el: portal.content.as_ref(),
            rect: content_rect,
        },
    );

    let node = tree.node_mut(id);
    node.children = vec![content_id];
    if let NodeKind::Portal(portal_node) = &mut node.kind {
        *portal_node.content = content_id;
    }

    if overlay_state.allow_root_overlays {
        let order = overlay_state.next_order();
        overlay_state.roots.push(OverlayRoot {
            id: content_id,
            overlay_id: None,
            layer: portal.layer,
            order,
            dismiss_policy: portal.dismiss_policy,
            on_dismiss: portal.on_close.clone(),
            backdrop: portal.backdrop,
            opacity: 1.0,
            captures_focus: portal.captures_focus,
            captures_pointer: portal.captures_pointer,
            copy_text: None,
            copy_zone: None,
            copy_feedback_active: false,
        });
    }

    id
}

pub(crate) fn reconcile_overlay_entries(ctx: &mut ReconcileCtx<'_>, overlays: &[OverlayEntry]) {
    let ReconcileCtx {
        tree,
        epoch,
        focus,
        overlay_state,
    } = ctx;
    let epoch = *epoch;
    let focus = *focus;
    if overlays.is_empty() || !overlay_state.allow_root_overlays {
        return;
    }

    let mut stacked_offsets: HashMap<ToastPlacement, u16> = HashMap::new();
    let mut stacked_entries: Vec<&OverlayEntry> = overlays
        .iter()
        .filter(|entry| matches!(entry.placement, OverlayPlacement::Stacked { .. }))
        .collect();
    stacked_entries.sort_by_key(|entry| Reverse(entry.order));

    let mut offset_by_id = HashMap::new();
    for entry in stacked_entries {
        let OverlayPlacement::Stacked { placement, gap, .. } = entry.placement else {
            continue;
        };
        let offset = stacked_offsets.entry(placement).or_insert(0);
        offset_by_id.insert(entry.id, *offset);
        let (_, height) = resolve_overlay_size(&entry.content, overlay_state.bounds, None);
        *offset = offset.saturating_add(height).saturating_add(gap);
    }

    for entry in overlays {
        let rect = match entry.placement {
            OverlayPlacement::Center { .. } => {
                resolve_center_rect(&entry.content, overlay_state.bounds, None)
            }
            OverlayPlacement::Stacked {
                placement, margin, ..
            } => {
                let offset = offset_by_id.get(&entry.id).copied().unwrap_or(0);
                resolve_stacked_rect(
                    &entry.content,
                    overlay_state.bounds,
                    placement,
                    margin,
                    offset,
                    None,
                )
            }
        };

        let id = reconcile_element(
            &mut ReconcileCtx {
                tree,
                epoch,
                focus,
                overlay_state,
            },
            ElementReconcile {
                reuse: None,
                parent: None,
                el: &entry.content,
                rect,
            },
        );

        let copy_text = if entry.pending_dismiss {
            None
        } else {
            entry.copy_text.clone()
        };
        let copy_zone =
            copy_text
                .as_ref()
                .zip(entry.copy_zone_right_padding)
                .map(|(_, right_padding)| {
                    crate::widgets::toast::copy_zone_with_right_padding(rect, right_padding)
                });

        overlay_state.roots.push(OverlayRoot {
            id,
            overlay_id: Some(entry.id),
            layer: entry.layer,
            order: entry.order,
            dismiss_policy: if entry.pending_dismiss {
                DismissPolicy::None
            } else {
                entry.dismiss_policy
            },
            on_dismiss: entry.on_dismiss.clone(),
            backdrop: entry.backdrop,
            opacity: entry.opacity(),
            captures_focus: if entry.pending_dismiss {
                false
            } else {
                entry.captures_focus
            },
            captures_pointer: entry.captures_pointer,
            copy_text,
            copy_zone,
            copy_feedback_active: entry.copy_feedback_active(),
        });
    }
}

pub(crate) fn collect_popover_overlay_roots(tree: &NodeTree, overlay_state: &mut OverlayState) {
    if !overlay_state.allow_root_overlays {
        return;
    }

    let mut seen = HashSet::new();
    for node in tree.iter() {
        let NodeKind::Popover(popover_node) = &node.kind else {
            continue;
        };

        if !popover_node.open
            || !matches!(popover_node.scope, crate::overlay::OverlayScope::RootPortal)
        {
            continue;
        }

        let content_id = *popover_node.content;
        if !tree.is_valid(content_id) || !seen.insert(content_id) {
            continue;
        }

        let order = overlay_state.next_order();
        overlay_state.roots.push(OverlayRoot {
            id: content_id,
            overlay_id: None,
            layer: OverlayLayer::Popover,
            order,
            dismiss_policy: if popover_node.on_close.is_some() {
                DismissPolicy::ClickOutsideOrEscape
            } else {
                DismissPolicy::None
            },
            on_dismiss: popover_node.on_close.clone(),
            backdrop: None,
            opacity: 1.0,
            captures_focus: false,
            captures_pointer: crate::overlay::PointerCapture::RectOnly,
            copy_text: None,
            copy_zone: None,
            copy_feedback_active: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::element::IntoElement;
    use crate::style::Length;
    use crate::widgets::{Frame, Text};

    /// Helper: create an element with a known minimum size by setting
    /// min_width / min_height constraints on a zero-content text node.
    fn sized_element(w: u16, h: u16) -> Element {
        Text::default()
            .min_width(Length::Px(w))
            .min_height(Length::Px(h))
    }

    #[test]
    fn center_rect_places_content_in_middle_of_bounds() {
        // Even-sized bounds: 80x24 container, 20x6 content → centered at (30, 9)
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 24,
        };
        let content = sized_element(20, 6);
        let r = resolve_center_rect(&content, bounds, None);
        assert_eq!(r.w, 20);
        assert_eq!(r.h, 6);
        assert_eq!(r.x, 30); // (80 - 20) / 2
        assert_eq!(r.y, 9); // (24 - 6) / 2

        // Odd remainder: 81x25 container, 20x6 content → floors the offset
        let bounds_odd = Rect {
            x: 0,
            y: 0,
            w: 81,
            h: 25,
        };
        let r2 = resolve_center_rect(&content, bounds_odd, None);
        assert_eq!(r2.w, 20);
        assert_eq!(r2.h, 6);
        assert_eq!(r2.x, 30); // (81 - 20) / 2 = 30 (integer division)
        assert_eq!(r2.y, 9); // (25 - 6) / 2 = 9

        // Non-zero origin: bounds offset at (5, 3)
        let bounds_offset = Rect {
            x: 5,
            y: 3,
            w: 80,
            h: 24,
        };
        let r3 = resolve_center_rect(&content, bounds_offset, None);
        assert_eq!(r3.x, 35); // 5 + 30
        assert_eq!(r3.y, 12); // 3 + 9
    }

    #[test]
    fn stacked_rect_positions_by_placement() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 24,
        };
        let content = sized_element(20, 3);

        // TopStart: 1 cell from left edge, 1+offset cells from top
        let r = resolve_stacked_rect(
            &content,
            bounds,
            ToastPlacement::TopStart,
            Padding::BORDER,
            0,
            None,
        );
        assert_eq!(r.x, 1);
        assert_eq!(r.y, 1);
        assert_eq!(r.w, 20);
        assert_eq!(r.h, 3);

        // BottomEnd: right-aligned with 1-cell margin, bottom-aligned
        let r = resolve_stacked_rect(
            &content,
            bounds,
            ToastPlacement::BottomEnd,
            Padding::BORDER,
            0,
            None,
        );
        // x = 0 + (80 - 20 - 1) = 59
        assert_eq!(r.x, 59);
        // y = 0 + (24 - 3 - 1) = 20
        assert_eq!(r.y, 20);

        // TopCenter: horizontally centered, top with 1-cell margin
        let r = resolve_stacked_rect(
            &content,
            bounds,
            ToastPlacement::TopCenter,
            Padding::BORDER,
            0,
            None,
        );
        assert_eq!(r.x, 30); // (80 - 20) / 2
        assert_eq!(r.y, 1);

        // With offset=5: TopStart shifts down by 5 extra rows
        let r = resolve_stacked_rect(
            &content,
            bounds,
            ToastPlacement::TopStart,
            Padding::BORDER,
            5,
            None,
        );
        assert_eq!(r.y, 6); // 1 + 5

        // BottomCenter with offset=4: shifts up from bottom
        let r = resolve_stacked_rect(
            &content,
            bounds,
            ToastPlacement::BottomCenter,
            Padding::BORDER,
            4,
            None,
        );
        assert_eq!(r.x, 30);
        // y = 0 + (24 - 3 - (1+4)) = 16
        assert_eq!(r.y, 16);
    }

    #[test]
    fn stacked_rect_respects_configured_margin() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 24,
        };
        let content = sized_element(20, 3);
        let margin = Padding::from((2, 4, 3, 5));

        let r = resolve_stacked_rect(&content, bounds, ToastPlacement::TopStart, margin, 0, None);
        assert_eq!(r.x, 5);
        assert_eq!(r.y, 2);

        let r = resolve_stacked_rect(&content, bounds, ToastPlacement::BottomEnd, margin, 0, None);
        assert_eq!(r.x, 56);
        assert_eq!(r.y, 18);

        let r = resolve_stacked_rect(
            &content,
            bounds,
            ToastPlacement::BottomCenter,
            margin,
            4,
            None,
        );
        assert_eq!(r.x, 30);
        assert_eq!(r.y, 14);
    }

    #[test]
    fn content_larger_than_bounds_is_clamped() {
        // Content wants 100x50, but bounds are only 40x10
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };
        let content = sized_element(100, 50);

        // Center: clamped to 40x10, placed at origin (no room to offset)
        let r = resolve_center_rect(&content, bounds, None);
        assert_eq!(r.w, 40);
        assert_eq!(r.h, 10);
        assert_eq!(r.x, 0);
        assert_eq!(r.y, 0);

        // Stacked TopStart: clamped size, positioned at 1-cell margin
        let r = resolve_stacked_rect(
            &content,
            bounds,
            ToastPlacement::TopStart,
            Padding::BORDER,
            0,
            None,
        );
        assert_eq!(r.w, 40);
        assert_eq!(r.h, 10);
        assert_eq!(r.x, 1);
        assert_eq!(r.y, 1);
    }

    #[test]
    fn zero_size_bounds_produces_zero_rect() {
        let bounds = Rect {
            x: 5,
            y: 10,
            w: 0,
            h: 0,
        };
        let content = sized_element(20, 6);

        let r = resolve_center_rect(&content, bounds, None);
        assert_eq!(r.w, 0);
        assert_eq!(r.h, 0);
        // Position should be at the bounds origin (no space to center)
        assert_eq!(r.x, 5);
        assert_eq!(r.y, 10);

        let r = resolve_stacked_rect(
            &content,
            bounds,
            ToastPlacement::BottomEnd,
            Padding::BORDER,
            0,
            None,
        );
        assert_eq!(r.w, 0);
        assert_eq!(r.h, 0);
    }

    #[test]
    fn center_rect_respects_percent_length_requests() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 24,
        };

        let content: Element = Frame::new()
            .border(true)
            .padding(1)
            .width(Length::Percent(50))
            .height(Length::Percent(50))
            .child(Text::new("x"))
            .into();

        let rect = resolve_center_rect(&content, bounds, None);
        assert_eq!(rect.w, 40);
        assert_eq!(rect.h, 12);
        assert_eq!(rect.x, 20);
        assert_eq!(rect.y, 6);
    }

    #[test]
    fn center_rect_respects_overlay_max_height_during_measurement() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 10,
        };

        let content: Element = Frame::new()
            .border(false)
            .height(Length::Auto)
            .child(Text::new("a\nb\nc\nd\ne\nf"))
            .into();
        let overlay_constraints = LayoutConstraints::default().max_height(Length::Percent(50));

        let rect = resolve_center_rect(&content, bounds, Some(&overlay_constraints));
        assert_eq!(rect.h, 5);
    }
}
