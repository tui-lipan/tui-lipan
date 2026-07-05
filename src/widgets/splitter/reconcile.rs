use crate::core::node::{NodeId, NodeKind};
use crate::layout::reconcile::{
    ElementReconcile, ReconcileCtx, reconcile_element, resolve_rect_with_auto,
};
use crate::style::{LayoutConstraints, Rect};
use crate::widgets::containers::reconcile::stack_reuse_plan;

use super::layout::{layout_splitter, measure_splitter};
use super::{Splitter, SplitterNode, resolve_weights};

pub(crate) struct SplitterReconcile<'a> {
    pub parent: NodeId,
    pub old_children: Vec<NodeId>,
    pub splitter: &'a Splitter,
    pub bounds: Rect,
    pub constraints: &'a LayoutConstraints,
}

pub(crate) fn reconcile_splitter(
    ctx: &mut ReconcileCtx<'_>,
    args: SplitterReconcile<'_>,
) -> Vec<NodeId> {
    let SplitterReconcile {
        parent,
        mut old_children,
        splitter,
        bounds,
        constraints,
    } = args;
    let plan = stack_reuse_plan(ctx.tree, &old_children, &splitter.children);

    old_children.clear();

    let resolved_max_w = constraints.max_w.and_then(|l| l.resolve_as_max(bounds.w));
    let resolved_max_h = constraints.max_h.and_then(|l| l.resolve_as_max(bounds.h));
    let (w, h) = measure_splitter(splitter, resolved_max_w, resolved_max_h);
    let rect = resolve_rect_with_auto(bounds, constraints, splitter.width, splitter.height, w, h);

    let (prev_weights, active_handle, prev_weights_nonce) = {
        let node = ctx.tree.node(parent);
        if let NodeKind::Splitter(existing) = &node.kind {
            (
                existing.weights.clone(),
                existing.active_handle,
                existing.weights_nonce,
            )
        } else {
            (Vec::new(), None, 0)
        }
    };

    let len = splitter.children.len();
    let explicit = &splitter.weights;
    let weights = if active_handle.is_some() {
        resolve_weights(explicit, &prev_weights, len)
    } else if splitter.weights_nonce > prev_weights_nonce
        && explicit.len() == len
        && !explicit.is_empty()
    {
        resolve_weights(explicit, &[], len)
    } else {
        resolve_weights(explicit, &prev_weights, len)
    };
    let layout = layout_splitter(splitter, &weights, rect);

    for (child, (reuse_id, child_rect)) in splitter
        .children
        .iter()
        .zip(plan.into_iter().zip(layout.pane_rects.iter().copied()))
    {
        let child_id = reconcile_element(
            ctx,
            ElementReconcile {
                reuse: reuse_id,
                parent: Some(parent),
                el: child,
                rect: child_rect,
            },
        );
        old_children.push(child_id);
    }

    let node = ctx.tree.node_mut(parent);
    node.rect = rect;
    node.kind = NodeKind::Splitter(SplitterNode {
        orientation: splitter.orientation,
        weights,
        weights_nonce: splitter.weights_nonce,
        split_id: splitter.split_id.clone(),
        on_resize: splitter.on_resize.clone(),
        min_size: splitter.min_size,
        join_frame: splitter.join_frame,
        handle_symbol: splitter.handle_symbol,
        handle_style: splitter.handle_style,
        handle_hover_style: splitter.handle_hover_style,
        handle_active_style: splitter.handle_active_style,
        handle_rects: layout.handle_rects,
        pane_sizes: layout.pane_sizes,
        active_handle,
    });

    old_children
}
