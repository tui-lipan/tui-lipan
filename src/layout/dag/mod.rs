//! Deterministic layered-DAG layout shared by diagram widgets.
//!
//! The module intentionally exposes a small, widget-agnostic IR: callers provide
//! node sizes and directed edges, and receive terminal-cell rectangles plus
//! orthogonal edge routes. Widget-specific labels, arrowheads, and decorations
//! remain outside the layout engine.

mod coords;
mod crossings;
mod layering;
mod ports;
mod routing;

use std::sync::Arc;

use crate::style::Rect;

pub(crate) use ports::DagPort;

/// Stable node identifier used by the DAG IR.
pub(crate) type DagNodeId = Arc<str>;

/// Minimum node size in terminal cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DagSize {
    pub(crate) w: u16,
    pub(crate) h: u16,
}

impl DagSize {
    pub(crate) const fn new(w: u16, h: u16) -> Self {
        Self { w, h }
    }
}

/// A node in the diagram layout graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DagNode {
    pub(crate) id: DagNodeId,
    pub(crate) min_size: DagSize,
    pub(crate) layer_hint: Option<usize>,
    pub(crate) group: Option<Arc<str>>,
}

impl DagNode {
    pub(crate) fn new(id: impl Into<Arc<str>>, min_size: DagSize) -> Self {
        Self {
            id: id.into(),
            min_size,
            layer_hint: None,
            group: None,
        }
    }
}

/// Semantic class of an edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum EdgeKind {
    #[default]
    Directed,
}

/// A directed edge in the diagram layout graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DagEdge {
    pub(crate) from: DagNodeId,
    pub(crate) to: DagNodeId,
    pub(crate) kind: EdgeKind,
    pub(crate) label: Option<Arc<str>>,
    pub(crate) head_from: bool,
    pub(crate) head_to: bool,
}

impl DagEdge {
    pub(crate) fn new(from: impl Into<Arc<str>>, to: impl Into<Arc<str>>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            kind: EdgeKind::Directed,
            label: None,
            head_from: false,
            head_to: true,
        }
    }

    pub(crate) fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub(crate) fn heads(mut self, head_from: bool, head_to: bool) -> Self {
        self.head_from = head_from;
        self.head_to = head_to;
        self
    }
}

/// Tuning knobs for layered DAG layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DagLayoutOptions {
    pub(crate) layer_gap: u16,
    pub(crate) node_gap: u16,
    pub(crate) margin_x: u16,
    pub(crate) margin_y: u16,
}

impl Default for DagLayoutOptions {
    fn default() -> Self {
        Self {
            layer_gap: 4,
            node_gap: 4,
            margin_x: 1,
            margin_y: 1,
        }
    }
}

/// Layout input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DagInput {
    pub(crate) nodes: Vec<DagNode>,
    pub(crate) edges: Vec<DagEdge>,
    pub(crate) options: DagLayoutOptions,
}

impl DagInput {
    pub(crate) fn new(nodes: Vec<DagNode>, edges: Vec<DagEdge>) -> Self {
        Self {
            nodes,
            edges,
            options: DagLayoutOptions::default(),
        }
    }
}

/// A laid-out node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PositionedNode {
    pub(crate) id: DagNodeId,
    pub(crate) rect: Rect,
    pub(crate) layer: usize,
    pub(crate) order: usize,
    pub(crate) group: Option<Arc<str>>,
}

/// A point in routed edge cell coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct DagPoint {
    pub(crate) x: i16,
    pub(crate) y: i16,
}

impl DagPoint {
    pub(crate) const fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }
}

/// A laid-out edge with orthogonal route points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RoutedEdge {
    pub(crate) from: DagNodeId,
    pub(crate) to: DagNodeId,
    pub(crate) kind: EdgeKind,
    pub(crate) label: Option<Arc<str>>,
    pub(crate) points: Vec<DagPoint>,
    pub(crate) from_port: DagPort,
    pub(crate) to_port: DagPort,
    pub(crate) reversed: bool,
    pub(crate) head_from: bool,
    pub(crate) head_to: bool,
}

/// Complete layout output.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DagOutput {
    pub(crate) positioned_nodes: Vec<PositionedNode>,
    pub(crate) routed_edges: Vec<RoutedEdge>,
    pub(crate) bounds: Rect,
}

#[derive(Clone, Debug)]
struct WorkingNode {
    spec_index: usize,
    layer: usize,
    order: usize,
    width: u16,
    height: u16,
}

#[derive(Clone, Debug)]
struct WorkingEdge {
    spec_index: usize,
    from: usize,
    to: usize,
    reversed: bool,
}

/// Compute a deterministic layered DAG layout.
pub(crate) fn compute(input: DagInput) -> DagOutput {
    if input.nodes.is_empty() {
        return DagOutput::default();
    }

    let mut nodes: Vec<_> = input
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| WorkingNode {
            spec_index: index,
            layer: node.layer_hint.unwrap_or(0),
            order: index,
            width: node.min_size.w.max(1),
            height: node.min_size.h.max(1),
        })
        .collect();
    let mut edges = layering::classify_edges(&input, nodes.len());
    layering::assign_layers(&mut nodes, &mut edges, &input.nodes);
    crossings::reduce_crossings(&mut nodes, &edges);

    let positioned_nodes = coords::assign_coordinates(&input, &nodes);
    let routed_edges = routing::route_edges(&input, &positioned_nodes, &edges);
    let bounds = coords::bounds(&positioned_nodes, &routed_edges);

    DagOutput {
        positioned_nodes,
        routed_edges,
        bounds,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::ports::PortSide;
    use super::*;

    fn node(id: &str) -> DagNode {
        DagNode::new(id, DagSize::new(3, 1))
    }

    #[test]
    fn assigns_layers_from_directed_edges() {
        let output = compute(DagInput::new(
            vec![node("a"), node("b"), node("c")],
            vec![DagEdge::new("a", "b"), DagEdge::new("b", "c")],
        ));

        let layers: Vec<_> = output
            .positioned_nodes
            .iter()
            .map(|node| (node.id.as_ref().to_owned(), node.layer))
            .collect();
        assert_eq!(
            layers,
            vec![("a".into(), 0), ("b".into(), 1), ("c".into(), 2)]
        );
    }

    #[test]
    fn keeps_order_deterministic_with_shared_successor() {
        let output = compute(DagInput::new(
            vec![node("a"), node("b"), node("c"), node("d")],
            vec![
                DagEdge::new("a", "c"),
                DagEdge::new("b", "c"),
                DagEdge::new("b", "d"),
            ],
        ));

        let first_layer: Vec<_> = output
            .positioned_nodes
            .iter()
            .filter(|node| node.layer == 0)
            .map(|node| node.id.as_ref())
            .collect();
        assert_eq!(first_layer, vec!["a", "b"]);
    }

    #[test]
    fn routes_edges_orthogonally_between_node_ports() {
        let output = compute(DagInput::new(
            vec![node("a"), node("b")],
            vec![DagEdge::new("a", "b")],
        ));

        let edge = &output.routed_edges[0];
        assert!(edge.points.len() >= 2);
        assert_eq!(edge.from_port.side, PortSide::South);
        assert_eq!(edge.to_port.side, PortSide::North);
        assert_eq!(edge.points[0], edge.from_port.point);
        assert_eq!(*edge.points.last().unwrap(), edge.to_port.point);
        for segment in edge.points.windows(2) {
            assert!(segment[0].x == segment[1].x || segment[0].y == segment[1].y);
        }
    }

    #[test]
    fn reclassifies_late_cross_link_as_feedback_to_keep_primary_stage_compact() {
        let output = compute(DagInput::new(
            vec![node("d"), node("f"), node("g"), node("j"), node("i")],
            vec![
                DagEdge::new("d", "f"),
                DagEdge::new("d", "g"),
                DagEdge::new("g", "j"),
                DagEdge::new("j", "f"),
                DagEdge::new("f", "i"),
            ],
        ));

        let layers = output
            .positioned_nodes
            .iter()
            .map(|node| (node.id.as_ref(), node.layer))
            .collect::<HashMap<_, _>>();
        assert_eq!(layers.get("f"), layers.get("g"));

        let feedback = output
            .routed_edges
            .iter()
            .find(|edge| edge.from.as_ref() == "j" && edge.to.as_ref() == "f")
            .expect("j->f edge present");
        assert!(
            feedback.reversed,
            "late cross-link should route as feedback"
        );
    }
}
