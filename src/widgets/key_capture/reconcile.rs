use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{SimpleLeafReconcile, reconcile_simple_leaf};
use crate::style::{LayoutConstraints, Rect};

use super::{KeyCapture, KeyCaptureNode, measure_key_capture};

pub fn reconcile_key_capture(
    tree: &mut NodeTree,
    id: NodeId,
    capture: &KeyCapture,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    reconcile_simple_leaf(
        tree,
        SimpleLeafReconcile {
            id,
            rect,
            constraints,
            width: capture.width,
            height: capture.height,
            measured: measure_key_capture(capture),
        },
        || NodeKind::KeyCapture(KeyCaptureNode::from(capture.clone())),
    )
}
