use std::sync::Arc;

use crate::widgets::common::diagram_text::wrap_label;
use crate::widgets::common::simple_diagram::{
    EndpointGlyph, SimpleDiagramBox, SimpleDiagramBoxShape, SimpleDiagramEdge, SimpleDiagramOutput,
    build_simple_diagram_output,
};

use super::{ClassDiagram, ClassMember, ClassRelationKind, ClassVisibility};

pub fn measure_class_diagram(diagram: &ClassDiagram) -> (u16, u16) {
    let output = build_class_diagram_output(diagram);
    (
        output.width.saturating_add(diagram.padding.horizontal()),
        output.height.saturating_add(diagram.padding.vertical()),
    )
}

pub(crate) fn build_class_diagram_output(diagram: &ClassDiagram) -> SimpleDiagramOutput {
    let boxes = class_boxes(diagram);
    let edges = diagram
        .relations
        .iter()
        .map(|relation| {
            let (from_glyph, to_glyph, dashed) = relation_glyphs(relation.kind);
            SimpleDiagramEdge {
                from: relation.from.clone(),
                to: relation.to.clone(),
                label: relation_label(relation),
                from_label: relation.multiplicity_from.clone(),
                to_label: relation.multiplicity_to.clone(),
                line_style: diagram.edge_style,
                label_style: diagram.label_style,
                dashed,
                from_glyph,
                to_glyph,
                prefer_vertical_backedge_labels: false,
            }
        })
        .collect::<Vec<_>>();
    build_simple_diagram_output(
        &boxes,
        &edges,
        diagram.node_padding,
        diagram.layer_gap,
        diagram.node_gap,
    )
}

pub(crate) fn class_boxes(diagram: &ClassDiagram) -> Vec<SimpleDiagramBox> {
    diagram
        .classes
        .iter()
        .map(|class| {
            let mut rows = Vec::new();
            push_wrapped(&mut rows, &class.name, diagram.max_node_width);
            let mut divider_after = rows
                .len()
                .checked_sub(1)
                .map(|title_last| vec![title_last])
                .unwrap_or_default();

            let attr_start = rows.len();
            for member in &class.attributes {
                let formatted = format_member(member, &diagram.theme);
                push_wrapped(&mut rows, &formatted, diagram.max_node_width);
            }
            if rows.len() > attr_start {
                divider_after.push(rows.len() - 1);
            }

            for member in &class.methods {
                let formatted = format_member(member, &diagram.theme);
                push_wrapped(&mut rows, &formatted, diagram.max_node_width);
            }
            SimpleDiagramBox {
                id: class.name.clone(),
                rows,
                divider_after,
                fill_style: diagram.class_style,
                border_style_fg: diagram.class_style,
                label_style: diagram.class_style,
                border_style: diagram.border_style,
                shape: SimpleDiagramBoxShape::Rect,
            }
        })
        .collect()
}

fn push_wrapped(rows: &mut Vec<Arc<str>>, text: &str, max_width: u16) {
    rows.extend(wrap_label(text, max_width).iter().cloned());
}

fn format_member(member: &ClassMember, theme: &super::ClassDiagramTheme) -> Arc<str> {
    let prefix = match member.visibility {
        ClassVisibility::Public => theme.public,
        ClassVisibility::Private => theme.private,
        ClassVisibility::Protected => theme.protected,
        ClassVisibility::Package => theme.package,
    };
    match &member.ty {
        Some(ty) => format!("{prefix}{}: {ty}", member.name).into(),
        None => format!("{prefix}{}", member.name).into(),
    }
}

fn relation_glyphs(kind: ClassRelationKind) -> (EndpointGlyph, EndpointGlyph, bool) {
    match kind {
        ClassRelationKind::Inheritance => (EndpointGlyph::None, EndpointGlyph::Triangle, false),
        ClassRelationKind::Realization => (EndpointGlyph::None, EndpointGlyph::Triangle, true),
        ClassRelationKind::Composition => (EndpointGlyph::Diamond, EndpointGlyph::None, false),
        ClassRelationKind::Aggregation => (EndpointGlyph::Circle, EndpointGlyph::None, false),
        ClassRelationKind::Dependency => (EndpointGlyph::None, EndpointGlyph::Arrow, true),
        ClassRelationKind::Association => (EndpointGlyph::None, EndpointGlyph::Arrow, false),
    }
}

fn relation_label(relation: &super::ClassRelation) -> Option<Arc<str>> {
    relation.label.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Padding;

    #[test]
    fn class_member_compartments_add_divider_rows() {
        let diagram = ClassDiagram::new()
            .attribute("User", ClassVisibility::Private, "id", "u64")
            .method("User", ClassVisibility::Public, "save", "() -> bool");

        let boxes = class_boxes(&diagram);
        let user = &boxes[0];

        assert_eq!(user.rows.len(), 3);
        assert_eq!(user.divider_after, vec![0, 1]);
        assert_eq!(user.min_size(Padding::default()).h, 7);
    }

    #[test]
    fn long_member_rows_wrap_before_measurement() {
        let diagram = ClassDiagram::new().max_node_width(8).attribute(
            "User",
            ClassVisibility::Private,
            "very_long_identifier",
            "String",
        );

        let boxes = class_boxes(&diagram);
        let user = &boxes[0];

        assert!(user.rows.len() > 2);
        assert!(user.rows.iter().all(|row| row.chars().count() <= 8));
    }
}
