use super::{GanttDiagram, GanttDiagramNode, measure_gantt_diagram};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{SimpleLeafReconcile, reconcile_simple_leaf};
use crate::style::{LayoutConstraints, Rect};

pub fn reconcile_gantt_diagram(
    tree: &mut NodeTree,
    id: NodeId,
    diagram: &GanttDiagram,
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
            measured: measure_gantt_diagram(diagram),
        },
        || NodeKind::GanttDiagram(Box::new(GanttDiagramNode::from(diagram.clone()))),
    )
}
