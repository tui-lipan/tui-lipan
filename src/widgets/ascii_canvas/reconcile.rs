use std::sync::Arc;

use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::apply_constraints;
use crate::style::{LayoutConstraints, Length, Rect};

use super::node::AsciiCanvasNode;
use super::{AsciiCanvas, measure_ascii_canvas};

pub fn reconcile_ascii_canvas(
    tree: &mut NodeTree,
    id: NodeId,
    canvas: &AsciiCanvas,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    let render_key = AsciiCanvasNode::render_key_for(canvas);
    let widget_key = AsciiCanvasNode::widget_key_for(canvas, render_key);

    let (w, h) = measure_ascii_canvas(canvas, Some(rect.w), Some(rect.h));

    let avail_w = rect.w;
    let avail_h = rect.h;
    let mut rect = rect;
    if matches!(canvas.width, Length::Auto) {
        rect.w = w.min(rect.w);
    }
    if matches!(canvas.height, Length::Auto) {
        rect.h = h.min(rect.h);
    }
    apply_constraints(&mut rect, constraints, avail_w, avail_h);

    let node = tree.node_mut(id);
    node.rect = rect;
    node.children.clear();

    if let NodeKind::AsciiCanvas(existing) = &mut node.kind {
        if existing.widget_key != widget_key {
            if existing.render_key != render_key {
                existing.lines = canvas.lines.clone();
                existing.cells = canvas.cells.clone();
                existing.grid_size = canvas.grid_size;
                existing.style = canvas.style;
                existing.background = canvas.background;
                existing.gradient = canvas.gradient;
                existing.color_map = canvas.color_map.clone();
                existing.fg_color_map = canvas.fg_color_map.clone();
                existing.bg_color_map = canvas.bg_color_map.clone();
                // Update sequence fields
                if let Some(ref seq) = canvas.sequence {
                    if existing
                        .sequence
                        .as_ref()
                        .is_none_or(|s| !Arc::ptr_eq(s, seq))
                    {
                        existing.sequence = Some(seq.clone());
                    }
                } else {
                    existing.sequence = None;
                }
                existing.current_frame = canvas.current_frame;
                existing.render_key = render_key;
            }
            existing.width = canvas.width;
            existing.height = canvas.height;
            existing.widget_key = widget_key;
        }
    } else {
        node.kind = NodeKind::AsciiCanvas(AsciiCanvasNode::from(canvas.clone()));
    }

    id
}
