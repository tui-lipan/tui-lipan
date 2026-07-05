use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::resolve_rect_with_auto;
use crate::style::{LayoutConstraints, Rect};

use super::{HexArea, HexAreaNode, measure_hex_area};

pub fn reconcile_hex_area(
    tree: &mut NodeTree,
    id: NodeId,
    hex_area: &HexArea,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    let (w, h) = measure_hex_area(hex_area);
    let rect = resolve_rect_with_auto(rect, constraints, hex_area.width, hex_area.height, w, h);

    let len = hex_area.bytes.len();
    let cursor = if len == 0 {
        0
    } else {
        hex_area.cursor.min(len.saturating_sub(1))
    };
    let anchor = if len == 0 {
        None
    } else {
        hex_area
            .anchor
            .map(|index| index.min(len.saturating_sub(1)))
    };

    let mut next = HexAreaNode::from(hex_area.clone());
    next.cursor = cursor;
    next.anchor = anchor;

    let node = tree.node_mut(id);
    node.rect = rect;
    node.children.clear();
    if let NodeKind::HexArea(existing) = &node.kind {
        next.pending_high_nibble = existing.pending_high_nibble;
    }
    node.kind = NodeKind::HexArea(next);

    id
}
