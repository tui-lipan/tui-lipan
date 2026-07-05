use std::sync::Arc;

use super::ErDiagram;
use crate::core::node::WidgetNode;
use crate::style::{Padding, Style};
use crate::widgets::common::simple_diagram::{
    SimpleDiagramBox, SimpleDiagramEdge, SimpleDiagramOutput,
};

#[derive(Clone)]
pub struct ErDiagramNode {
    pub(crate) style: Style,
    pub(crate) padding: Padding,
    pub(crate) node_padding: Padding,
    pub(crate) boxes: Arc<[SimpleDiagramBox]>,
    pub(crate) edges: Arc<[SimpleDiagramEdge]>,
    pub(crate) output: Arc<SimpleDiagramOutput>,
}

impl WidgetNode for ErDiagramNode {}

impl From<ErDiagram> for ErDiagramNode {
    fn from(value: ErDiagram) -> Self {
        let boxes = super::layout::er_boxes(&value);
        let edges = super::layout::er_edges(&value);
        let output = crate::widgets::common::simple_diagram::build_simple_diagram_output(
            &boxes,
            &edges,
            value.node_padding,
            value.layer_gap,
            value.node_gap,
        );
        Self {
            style: value.style,
            padding: value.padding,
            node_padding: value.node_padding,
            boxes: boxes.into(),
            edges: edges.into(),
            output: Arc::new(output),
        }
    }
}
