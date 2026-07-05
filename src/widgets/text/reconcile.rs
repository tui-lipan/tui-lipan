use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::resolve_rect_with_auto;
use crate::style::{LayoutConstraints, Rect};

use super::{Text, TextNode, measure_text_constrained};

pub fn reconcile_text(
    tree: &mut NodeTree,
    id: NodeId,
    text: &Text,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    let render_key = TextNode::render_key_for(text);
    let widget_key = TextNode::widget_key_for(text, render_key);

    let (w, h) = measure_text_constrained(text, Some(rect.w));
    let rect = resolve_rect_with_auto(rect, constraints, text.width, text.height, w, h);

    let node = tree.node_mut(id);
    node.rect = rect;
    node.children.clear();
    if let NodeKind::Text(existing) = &mut node.kind {
        if existing.widget_key != widget_key {
            if existing.render_key != render_key {
                existing.spans = text.spans.clone();
                existing.style = text.style;
                existing.overflow = text.overflow;
                existing.render_key = render_key;
            }
            existing.widget_key = widget_key;
        }
    } else {
        node.kind = NodeKind::Text(TextNode::from(text.clone()));
    }

    id
}
