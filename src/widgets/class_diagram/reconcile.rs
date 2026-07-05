use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{SimpleLeafReconcile, reconcile_simple_leaf};
use crate::style::{LayoutConstraints, Rect};

use super::{ClassDiagram, ClassDiagramNode, measure_class_diagram};

pub fn reconcile_class_diagram(
    tree: &mut NodeTree,
    id: NodeId,
    diagram: &ClassDiagram,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    reconcile_simple_leaf(
        tree,
        SimpleLeafReconcile {
            id,
            rect,
            constraints,
            width: diagram.width,
            height: diagram.height,
            measured: measure_class_diagram(diagram),
        },
        || NodeKind::ClassDiagram(Box::new(ClassDiagramNode::from(diagram.clone()))),
    )
}
