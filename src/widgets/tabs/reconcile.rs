use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{SimpleLeafReconcile, reconcile_simple_leaf};
use crate::style::{LayoutConstraints, Rect};

use super::{Tabs, measure_tabs};

pub fn reconcile_tabs(
    tree: &mut NodeTree,
    id: NodeId,
    tabs: &Tabs,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    // Tabs is a leaf widget at reconcile-time (labels live in node state), so
    // there is no multi-child reuse path here.
    reconcile_simple_leaf(
        tree,
        SimpleLeafReconcile {
            id,
            rect,
            constraints,
            width: tabs.width,
            height: tabs.height,
            measured: measure_tabs(tabs),
        },
        || NodeKind::Tabs(tabs.clone().into()),
    )
}
