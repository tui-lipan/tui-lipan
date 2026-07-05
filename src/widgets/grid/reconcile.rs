use std::rc::Rc;

use crate::core::node::{NodeId, NodeKind};
use crate::layout::hash::grid_layout_hash;
use crate::layout::reconcile::{ElementReconcile, ReconcileCtx, reconcile_element};
use crate::style::Rect;
use crate::widgets::containers::reconcile::stack_reuse_plan;

use super::layout::layout_grid;
use super::node::GridLayoutCache;
use crate::widgets::Grid;

pub(crate) struct GridReconcile<'a> {
    pub parent: NodeId,
    pub old_children: Vec<NodeId>,
    pub grid: &'a Grid,
    pub bounds: Rect,
}

pub(crate) fn reconcile_grid(ctx: &mut ReconcileCtx<'_>, args: GridReconcile<'_>) -> Vec<NodeId> {
    let GridReconcile {
        parent,
        mut old_children,
        grid,
        bounds,
    } = args;
    let focus = ctx.focus;
    let child_refs: Vec<&crate::core::element::Element> =
        grid.items.iter().map(|i| &i.element).collect();
    let plan = stack_reuse_plan(ctx.tree, &old_children, &child_refs);
    old_children.clear();

    let layout_hash = grid_layout_hash(grid, bounds, focus);
    let rects = match layout_hash {
        Some(hash) => {
            let cached = {
                let node = ctx.tree.node(parent);
                if let NodeKind::Grid(node) = &node.kind {
                    node.layout_cache
                        .as_ref()
                        .filter(|cache| {
                            cache.bounds == bounds
                                && cache.layout_hash == hash
                                && cache.child_rects.len() == grid.items.len()
                        })
                        .map(|cache| Rc::clone(&cache.child_rects))
                } else {
                    None
                }
            };
            cached.unwrap_or_else(|| Rc::new(layout_grid(&grid.props, &grid.items, bounds)))
        }
        None => Rc::new(layout_grid(&grid.props, &grid.items, bounds)),
    };

    for (item, (reuse_id, rect)) in grid
        .items
        .iter()
        .zip(plan.into_iter().zip(rects.iter().copied()))
    {
        let child_id = reconcile_element(
            ctx,
            ElementReconcile {
                reuse: reuse_id,
                parent: Some(parent),
                el: &item.element,
                rect,
            },
        );
        old_children.push(child_id);
    }

    {
        let node = ctx.tree.node_mut(parent);
        if let NodeKind::Grid(node) = &mut node.kind {
            node.layout_cache = layout_hash.map(|hash| GridLayoutCache {
                bounds,
                layout_hash: hash,
                child_rects: Rc::clone(&rects),
            });
        }
    }

    old_children
}
