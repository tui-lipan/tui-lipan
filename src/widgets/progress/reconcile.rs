use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{SimpleLeafReconcile, reconcile_simple_leaf};
use crate::style::{LayoutConstraints, Rect};

use super::{ProgressBar, ProgressNode, measure_progress_bar};

pub fn reconcile_progress_bar(
    tree: &mut NodeTree,
    id: NodeId,
    progress: &ProgressBar,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    reconcile_simple_leaf(
        tree,
        SimpleLeafReconcile {
            id,
            rect,
            constraints,
            width: progress.width,
            height: progress.height,
            measured: measure_progress_bar(progress),
        },
        || NodeKind::ProgressBar(ProgressNode::from(progress.clone())),
    )
}
