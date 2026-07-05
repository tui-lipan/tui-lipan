use crate::core::node::{NodeId, NodeKind};
use crate::layout::measure::min_size_constrained;
use crate::layout::reconcile::{
    ElementReconcile, ReconcileCtx, apply_constraints, reconcile_element,
};
use crate::layout::tag::can_reuse;
use crate::style::{LayoutConstraints, Length, Rect};

use super::{Divider, DividerNode, Orientation, measure_divider};

pub(crate) struct DividerReconcile<'a> {
    pub id: NodeId,
    pub divider: &'a Divider,
    pub rect: Rect,
    pub constraints: &'a LayoutConstraints,
}

pub fn reconcile_divider(ctx: &mut ReconcileCtx<'_>, args: DividerReconcile<'_>) -> NodeId {
    let DividerReconcile {
        id,
        divider,
        rect,
        constraints,
    } = args;
    let (w, h) = measure_divider(divider);

    let avail_w = rect.w;
    let avail_h = rect.h;
    let mut rect = rect;
    if matches!(divider.width, Length::Auto) {
        rect.w = w.min(rect.w);
    }
    if matches!(divider.height, Length::Auto) {
        rect.h = h.min(rect.h);
    }
    apply_constraints(&mut rect, constraints, avail_w, avail_h);

    let old_children = {
        let node = ctx.tree.node_mut(id);
        node.rect = rect;
        node.kind = NodeKind::Divider(DividerNode::from(divider.clone()));
        std::mem::take(&mut node.children)
    };

    let reuse_child = if divider.orientation == Orientation::Horizontal {
        divider.label.as_deref().and_then(|label| {
            old_children
                .iter()
                .copied()
                .find(|id| ctx.tree.is_valid(*id) && can_reuse(ctx.tree.node(*id), label))
        })
    } else {
        None
    };

    let mut new_children = old_children;
    new_children.clear();

    if divider.orientation == Orientation::Horizontal
        && let Some(label) = divider.label.as_deref()
    {
        let padding = divider.label_padding;
        let max_label_w = rect.w.saturating_sub(padding.saturating_mul(2));
        if max_label_w > 0 && rect.h > 0 {
            let (min_label_w, _) = min_size_constrained(label, Some(max_label_w), Some(1));
            let label_w = match divider.label_alignment {
                crate::style::Align::Stretch => max_label_w,
                _ => min_label_w,
            };

            if label_w > 0 {
                let label_x = match divider.label_alignment {
                    crate::style::Align::Start | crate::style::Align::Stretch => {
                        rect.x.saturating_add(padding as i16)
                    }
                    crate::style::Align::Center => rect
                        .x
                        .saturating_add((rect.w.saturating_sub(label_w) / 2) as i16),
                    crate::style::Align::End => rect
                        .x
                        .saturating_add(rect.w.saturating_sub(label_w) as i16)
                        .saturating_sub(padding as i16),
                };

                let child_rect = Rect {
                    x: label_x,
                    y: rect.y,
                    w: label_w,
                    h: 1,
                };

                let child_id = reconcile_element(
                    ctx,
                    ElementReconcile {
                        reuse: reuse_child,
                        parent: Some(id),
                        el: label,
                        rect: child_rect,
                    },
                );
                new_children.push(child_id);
            }
        }
    }

    let node = ctx.tree.node_mut(id);
    node.children = new_children;

    id
}
