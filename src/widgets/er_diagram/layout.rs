use super::{ErCardinality, ErDiagram};
use crate::widgets::common::diagram_text::wrap_label;
use crate::widgets::common::simple_diagram::{
    EndpointGlyph, SimpleDiagramBox, SimpleDiagramBoxShape, SimpleDiagramEdge, SimpleDiagramOutput,
    build_simple_diagram_output,
};
use std::sync::Arc;
pub fn measure_er_diagram(diagram: &ErDiagram) -> (u16, u16) {
    let output = build_er_diagram_output(diagram);
    (
        output.width.saturating_add(diagram.padding.horizontal()),
        output.height.saturating_add(diagram.padding.vertical()),
    )
}
pub(crate) fn build_er_diagram_output(diagram: &ErDiagram) -> SimpleDiagramOutput {
    let boxes = er_boxes(diagram);
    let edges = er_edges(diagram);
    build_simple_diagram_output(
        &boxes,
        &edges,
        diagram.node_padding,
        diagram.layer_gap,
        diagram.node_gap,
    )
}
pub(crate) fn er_boxes(diagram: &ErDiagram) -> Vec<SimpleDiagramBox> {
    diagram
        .entities
        .iter()
        .map(|entity| {
            let mut rows = Vec::new();
            push_wrapped(&mut rows, &entity.name, diagram.max_node_width);
            let title_last = rows.len().saturating_sub(1);
            for attribute in &entity.attributes {
                let mut keys = Vec::new();
                if attribute.pk {
                    keys.push(diagram.theme.pk);
                }
                if attribute.fk {
                    keys.push(diagram.theme.fk);
                }
                if attribute.uk {
                    keys.push(diagram.theme.uk);
                }
                let formatted = if keys.is_empty() {
                    format!("{} {}", attribute.ty, attribute.name)
                } else {
                    format!("{} {} {}", attribute.ty, attribute.name, keys.join(","))
                };
                push_wrapped(&mut rows, &formatted, diagram.max_node_width);
            }
            SimpleDiagramBox {
                id: entity.name.clone(),
                rows,
                divider_after: vec![title_last],
                fill_style: diagram.entity_style,
                border_style_fg: diagram.entity_style,
                label_style: diagram.entity_style,
                border_style: diagram.border_style,
                shape: SimpleDiagramBoxShape::Rect,
            }
        })
        .collect()
}

fn push_wrapped(rows: &mut Vec<Arc<str>>, text: &str, max_width: u16) {
    rows.extend(wrap_label(text, max_width).iter().cloned());
}

pub(crate) fn er_edges(diagram: &ErDiagram) -> Vec<SimpleDiagramEdge> {
    diagram
        .relations
        .iter()
        .map(|r| SimpleDiagramEdge {
            from: r.left.clone(),
            to: r.right.clone(),
            label: r.label.clone(),
            from_label: None,
            to_label: None,
            line_style: diagram.edge_style,
            label_style: diagram.label_style,
            dashed: false,
            from_glyph: cardinality_glyph(r.left_cardinality),
            to_glyph: cardinality_glyph(r.right_cardinality),
            prefer_vertical_backedge_labels: true,
        })
        .collect()
}
fn cardinality_glyph(cardinality: ErCardinality) -> EndpointGlyph {
    match cardinality {
        ErCardinality::ZeroOrOne => EndpointGlyph::CrowZeroOrOne,
        ErCardinality::ExactlyOne => EndpointGlyph::CrowExactlyOne,
        ErCardinality::ZeroOrMore => EndpointGlyph::CrowZeroOrMore,
        ErCardinality::OneOrMore => EndpointGlyph::CrowOneOrMore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{ErAttribute, ErEntity};

    #[test]
    fn entity_name_wraps_before_measurement() {
        let diagram = ErDiagram::new()
            .entities([ErEntity::new("CUSTOMER_ACCOUNT_PROFILE")])
            .max_node_width(8);

        let boxes = er_boxes(&diagram);
        let entity = &boxes[0];

        assert!(entity.rows.len() > 1);
        assert!(entity.rows.iter().all(|row| row.chars().count() <= 8));
        assert_eq!(entity.divider_after, vec![entity.rows.len() - 1]);
    }

    #[test]
    fn entity_attribute_rows_wrap_before_measurement() {
        let diagram = ErDiagram::new()
            .entities([ErEntity::new("ORDER")
                .attribute(ErAttribute::new("varchar", "customer_display_name").uk())])
            .max_node_width(10);

        let boxes = er_boxes(&diagram);
        let entity = &boxes[0];

        assert!(entity.rows.len() > 2);
        assert!(entity.rows.iter().all(|row| row.chars().count() <= 10));
    }
}
