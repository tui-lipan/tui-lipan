//! Internal status bar layout primitive.

use crate::core::element::{Element, ElementKind};
use crate::core::node::{NodeId, NodeKind, WidgetNode};
use crate::layout::measure::min_size_constrained;
use crate::layout::reconcile::{
    ElementReconcile, ReconcileCtx, reconcile_element, resolve_rect_with_auto,
};
use crate::layout::tag::can_reuse;
use crate::style::{LayoutConstraints, Length, Padding, Rect, Style};

#[derive(Clone)]
pub(crate) struct StatusBarLayout {
    pub(crate) left: Box<Element>,
    pub(crate) center: Box<Element>,
    pub(crate) right: Box<Element>,
    pub(crate) style: Style,
    pub(crate) padding: Padding,
    pub(crate) gap: u16,
    pub(crate) width: Length,
    pub(crate) height: Length,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StatusBarLayoutNode {
    pub(crate) style: Style,
    pub(crate) padding: Padding,
}

impl WidgetNode for StatusBarLayoutNode {}

impl From<StatusBarLayout> for Element {
    fn from(layout: StatusBarLayout) -> Self {
        Element::new(ElementKind::StatusBarLayout(layout))
    }
}

pub(crate) fn measure_status_bar_layout(
    layout: &StatusBarLayout,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    let inner_max_w = max_w.map(|w| w.saturating_sub(layout.padding.horizontal()));
    let inner_max_h = max_h.map(|h| h.saturating_sub(layout.padding.vertical()));

    let (left_w, left_h) = min_size_constrained(&layout.left, inner_max_w, inner_max_h);
    let (center_w, center_h) = min_size_constrained(&layout.center, inner_max_w, inner_max_h);
    let (right_w, right_h) = min_size_constrained(&layout.right, inner_max_w, inner_max_h);

    let side_w = left_w.max(right_w);
    let width = layout
        .padding
        .horizontal()
        .saturating_add(center_w)
        .saturating_add(layout.gap.saturating_mul(2))
        .saturating_add(side_w.saturating_mul(2));
    let height = layout
        .padding
        .vertical()
        .saturating_add(left_h.max(center_h).max(right_h));

    (width, height)
}

pub(crate) struct StatusBarLayoutReconcile<'a> {
    pub id: NodeId,
    pub layout: &'a StatusBarLayout,
    pub rect: Rect,
    pub constraints: &'a LayoutConstraints,
}

pub(crate) fn reconcile_status_bar_layout(
    ctx: &mut ReconcileCtx<'_>,
    args: StatusBarLayoutReconcile<'_>,
) -> NodeId {
    let StatusBarLayoutReconcile {
        id,
        layout,
        rect,
        constraints,
    } = args;
    let (min_w, min_h) = measure_status_bar_layout(layout, Some(rect.w), Some(rect.h));
    let rect = resolve_rect_with_auto(rect, constraints, layout.width, layout.height, min_w, min_h);

    let old_children = {
        let node = ctx.tree.node_mut(id);
        node.rect = rect;
        node.kind = NodeKind::StatusBarLayout(StatusBarLayoutNode {
            style: layout.style,
            padding: layout.padding,
        });
        std::mem::take(&mut node.children)
    };

    let inner = rect.inner(false, layout.padding);
    let (center_w, _) = min_size_constrained(&layout.center, Some(inner.w), Some(inner.h));
    let center_w = center_w.min(inner.w);
    let center_x = inner
        .x
        .saturating_add((inner.w.saturating_sub(center_w) / 2) as i16);
    let center_right = center_x.saturating_add(center_w as i16);
    let inner_right = inner.x.saturating_add(inner.w as i16);
    let gap = layout.gap.min(inner.w) as i16;
    let left_end = center_x.saturating_sub(gap).max(inner.x);
    let right_start = center_right.saturating_add(gap).min(inner_right);

    let left_rect = Rect {
        x: inner.x,
        y: inner.y,
        w: left_end.saturating_sub(inner.x).max(0) as u16,
        h: inner.h,
    };
    let center_rect = Rect {
        x: center_x,
        y: inner.y,
        w: center_w,
        h: inner.h,
    };
    let right_rect = Rect {
        x: right_start,
        y: inner.y,
        w: inner_right.saturating_sub(right_start).max(0) as u16,
        h: inner.h,
    };

    let mut claimed_reuse_ids = Vec::with_capacity(3);
    let mut find_reuse = |el: &Element| {
        let reuse_id = old_children.iter().copied().find(|cid| {
            ctx.tree.is_valid(*cid)
                && !claimed_reuse_ids.contains(cid)
                && can_reuse(ctx.tree.node(*cid), el)
        });
        if let Some(id) = reuse_id {
            claimed_reuse_ids.push(id);
        }
        reuse_id
    };

    let reuse_left = find_reuse(&layout.left);
    let reuse_center = find_reuse(&layout.center);
    let reuse_right = find_reuse(&layout.right);

    let mut new_children = old_children;
    new_children.clear();

    for (element, reuse, child_rect) in [
        (layout.left.as_ref(), reuse_left, left_rect),
        (layout.center.as_ref(), reuse_center, center_rect),
        (layout.right.as_ref(), reuse_right, right_rect),
    ] {
        let child_id = reconcile_element(
            ctx,
            ElementReconcile {
                reuse,
                parent: Some(id),
                el: element,
                rect: child_rect,
            },
        );
        new_children.push(child_id);
    }

    ctx.tree.node_mut(id).children = new_children;
    id
}
