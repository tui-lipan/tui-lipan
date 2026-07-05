use std::sync::Arc;

use crate::core::node::WidgetNode;
use crate::style::{Padding, Style};
use crate::widgets::common::simple_diagram::{
    SimpleDiagramBox, SimpleDiagramEdge, SimpleDiagramOutput,
};

use super::ClassDiagram;

#[derive(Clone)]
pub struct ClassDiagramNode {
    pub(crate) style: Style,
    pub(crate) padding: Padding,
    pub(crate) node_padding: Padding,
    pub(crate) boxes: Arc<[SimpleDiagramBox]>,
    pub(crate) edges: Arc<[SimpleDiagramEdge]>,
    pub(crate) output: Arc<SimpleDiagramOutput>,
}

impl WidgetNode for ClassDiagramNode {}

impl From<ClassDiagram> for ClassDiagramNode {
    fn from(value: ClassDiagram) -> Self {
        let boxes = super::layout::class_boxes(&value);
        let edges = value
            .relations
            .iter()
            .map(|relation| {
                let (from_glyph, to_glyph, dashed) = match relation.kind {
                    super::ClassRelationKind::Inheritance => (
                        crate::widgets::common::simple_diagram::EndpointGlyph::None,
                        crate::widgets::common::simple_diagram::EndpointGlyph::Triangle,
                        false,
                    ),
                    super::ClassRelationKind::Realization => (
                        crate::widgets::common::simple_diagram::EndpointGlyph::None,
                        crate::widgets::common::simple_diagram::EndpointGlyph::Triangle,
                        true,
                    ),
                    super::ClassRelationKind::Composition => (
                        crate::widgets::common::simple_diagram::EndpointGlyph::Diamond,
                        crate::widgets::common::simple_diagram::EndpointGlyph::None,
                        false,
                    ),
                    super::ClassRelationKind::Aggregation => (
                        crate::widgets::common::simple_diagram::EndpointGlyph::Circle,
                        crate::widgets::common::simple_diagram::EndpointGlyph::None,
                        false,
                    ),
                    super::ClassRelationKind::Dependency => (
                        crate::widgets::common::simple_diagram::EndpointGlyph::None,
                        crate::widgets::common::simple_diagram::EndpointGlyph::Arrow,
                        true,
                    ),
                    super::ClassRelationKind::Association => (
                        crate::widgets::common::simple_diagram::EndpointGlyph::None,
                        crate::widgets::common::simple_diagram::EndpointGlyph::Arrow,
                        false,
                    ),
                };
                SimpleDiagramEdge {
                    from: relation.from.clone(),
                    to: relation.to.clone(),
                    label: relation.label.clone(),
                    from_label: relation.multiplicity_from.clone(),
                    to_label: relation.multiplicity_to.clone(),
                    line_style: value.edge_style,
                    label_style: value.label_style,
                    dashed,
                    from_glyph,
                    to_glyph,
                    prefer_vertical_backedge_labels: false,
                }
            })
            .collect::<Vec<_>>();
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
