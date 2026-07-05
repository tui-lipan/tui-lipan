use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{SimpleLeafReconcile, reconcile_simple_leaf};
use crate::style::{LayoutConstraints, Rect};

use super::{Spacer, SpacerNode, measure_spacer};

pub fn reconcile_spacer(
    tree: &mut NodeTree,
    id: NodeId,
    spacer: &Spacer,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    reconcile_simple_leaf(
        tree,
        SimpleLeafReconcile {
            id,
            rect,
            constraints,
            width: spacer.width,
            height: spacer.height,
            measured: measure_spacer(spacer),
        },
        || NodeKind::Spacer(SpacerNode),
    )
}
