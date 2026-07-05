use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{SimpleLeafReconcile, reconcile_simple_leaf};
use crate::style::{LayoutConstraints, Rect};

use super::{Checkbox, CheckboxNode, measure_checkbox};

pub fn reconcile_checkbox(
    tree: &mut NodeTree,
    id: NodeId,
    checkbox: &Checkbox,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    reconcile_simple_leaf(
        tree,
        SimpleLeafReconcile {
            id,
            rect,
            constraints,
            width: checkbox.width,
            height: checkbox.height,
            measured: measure_checkbox(checkbox),
        },
        || NodeKind::Checkbox(CheckboxNode::from(checkbox.clone())),
    )
}
