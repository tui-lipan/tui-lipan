use super::{StateDiagram, StateKind};
use crate::widgets::common::diagram_text::wrap_label;
use crate::widgets::common::simple_diagram::{
    EndpointGlyph, SimpleDiagramBox, SimpleDiagramBoxShape, SimpleDiagramEdge, SimpleDiagramOutput,
    build_simple_diagram_output,
};
use std::sync::Arc;

pub fn measure_state_diagram(diagram: &StateDiagram) -> (u16, u16) {
    let output = build_state_diagram_output(diagram);
    (
        output.width.saturating_add(diagram.padding.horizontal()),
        output.height.saturating_add(diagram.padding.vertical()),
    )
}
pub(crate) fn build_state_diagram_output(diagram: &StateDiagram) -> SimpleDiagramOutput {
    let boxes = state_boxes(diagram);
    let edges = state_edges(diagram);
    build_simple_diagram_output(
        &boxes,
        &edges,
        diagram.node_padding,
        diagram.layer_gap,
        diagram.node_gap,
    )
}
pub(crate) fn state_boxes(diagram: &StateDiagram) -> Vec<SimpleDiagramBox> {
    diagram
        .states
        .iter()
        .map(|state| {
            let mut rows = Vec::new();
            match state.kind {
                StateKind::Start => rows.push(diagram.theme.start.to_string().into()),
                StateKind::End => rows.push(diagram.theme.end.to_string().into()),
                StateKind::Choice => rows.push(diagram.theme.choice.to_string().into()),
                StateKind::Fork | StateKind::Join => {
                    rows.push(diagram.theme.fork_join.to_string().repeat(3).into())
                }
                StateKind::State => {
                    push_wrapped(&mut rows, &state.label, diagram.max_node_width);
                    if let Some(entry) = &state.entry {
                        push_wrapped(
                            &mut rows,
                            &format!("entry / {entry}"),
                            diagram.max_node_width,
                        );
                    }
                    if let Some(exit) = &state.exit {
                        push_wrapped(&mut rows, &format!("exit / {exit}"), diagram.max_node_width);
                    }
                }
            }
            SimpleDiagramBox {
                id: state.id.clone(),
                rows,
                divider_after: Vec::new(),
                fill_style: diagram.state_style,
                border_style_fg: diagram.state_style,
                label_style: diagram.state_style,
                border_style: diagram.border_style,
                shape: SimpleDiagramBoxShape::Rect,
            }
        })
        .collect()
}

fn push_wrapped(rows: &mut Vec<Arc<str>>, text: &str, max_width: u16) {
    rows.extend(wrap_label(text, max_width).iter().cloned());
}

pub(crate) fn state_edges(diagram: &StateDiagram) -> Vec<SimpleDiagramEdge> {
    diagram
        .transitions
        .iter()
        .map(|t| {
            let label: Option<Arc<str>> = match (&t.label, &t.guard) {
                (Some(label), Some(guard)) => Some(format!("{label} [{guard}]").into()),
                (Some(label), None) => Some(label.clone()),
                (None, Some(guard)) => Some(format!("[{guard}]").into()),
                (None, None) => None,
            };
            SimpleDiagramEdge {
                from: t.from.clone(),
                to: t.to.clone(),
                label,
                from_label: None,
                to_label: None,
                line_style: diagram.edge_style,
                label_style: diagram.label_style,
                dashed: false,
                from_glyph: EndpointGlyph::None,
                to_glyph: EndpointGlyph::Arrow,
                prefer_vertical_backedge_labels: true,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::StateSpec;

    #[test]
    fn state_rows_wrap_before_measurement() {
        let diagram = StateDiagram::new()
            .states([StateSpec::new("processing").label("Waiting for external approval")])
            .max_node_width(10);

        let boxes = state_boxes(&diagram);
        let state = &boxes[0];

        assert!(state.rows.len() > 1);
        assert!(state.rows.iter().all(|row| row.chars().count() <= 10));
    }

    #[test]
    fn state_entry_exit_rows_wrap_before_measurement() {
        let diagram = StateDiagram::new()
            .states([StateSpec::new("processing")
                .entry("hydrate cache before rendering")
                .exit("flush metrics after update")])
            .max_node_width(12);

        let boxes = state_boxes(&diagram);
        let state = &boxes[0];

        assert!(state.rows.len() > 3);
        assert!(state.rows.iter().all(|row| row.chars().count() <= 12));
    }
}
