use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::{Length, Rect};

use super::{Spinner, SpinnerNode, measure_spinner};

pub fn reconcile_spinner(tree: &mut NodeTree, id: NodeId, spinner: &Spinner, rect: Rect) -> NodeId {
    let (w, h) = measure_spinner(spinner);

    let mut rect = rect;
    if matches!(spinner.width, Length::Auto) {
        rect.w = w.min(rect.w);
    }
    if matches!(spinner.height, Length::Auto) {
        rect.h = h.min(rect.h);
    }

    let node = tree.node_mut(id);
    // If spinner frame is None (auto), preserve existing frame if reusing.
    let next_frame = if let Some(frame) = spinner.frame {
        frame
    } else if let NodeKind::Spinner(SpinnerNode { frame, .. }) = &node.kind {
        *frame
    } else {
        0
    };

    node.rect = rect;
    node.children.clear();
    let mut spinner_node = SpinnerNode::from(spinner.clone());
    spinner_node.frame = next_frame;
    node.kind = NodeKind::Spinner(spinner_node);

    id
}
