use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{SimpleLeafReconcile, reconcile_simple_leaf};
use crate::style::{LayoutConstraints, Rect};

use super::{Button, ButtonNode, measure_button};

pub fn reconcile_button(
    tree: &mut NodeTree,
    id: NodeId,
    button: &Button,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    reconcile_simple_leaf(
        tree,
        SimpleLeafReconcile {
            id,
            rect,
            constraints,
            width: button.width,
            height: button.height,
            measured: measure_button(button),
        },
        || NodeKind::Button(ButtonNode::from(button.clone())),
    )
}
