use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{SimpleLeafReconcile, reconcile_simple_leaf};
use crate::style::{LayoutConstraints, Rect};

use super::{Slider, SliderNode, measure_slider};

pub fn reconcile_slider(
    tree: &mut NodeTree,
    id: NodeId,
    slider: &Slider,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    reconcile_simple_leaf(
        tree,
        SimpleLeafReconcile {
            id,
            rect,
            constraints,
            width: slider.width,
            height: slider.height,
            measured: measure_slider(slider),
        },
        || NodeKind::Slider(SliderNode::from(slider.clone())),
    )
}
