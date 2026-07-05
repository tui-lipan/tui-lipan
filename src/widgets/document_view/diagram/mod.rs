//! Pure Mermaid diagram IR and deterministic ASCII rasterization for `DocumentView`.

#![allow(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;

use crate::style::text::Span;
use crate::style::{BorderStyle, Color, Padding, Rect, Style};
use crate::utils::gradient::ColorGradient;
use crate::widgets::common::simple_diagram::{
    EndpointGlyph, SimpleDiagramBox, SimpleDiagramBoxShape, SimpleDiagramEdge, SimpleDiagramOutput,
    build_simple_diagram_output, normalized_dividers,
};
use crate::widgets::gantt_diagram::{
    GanttRenderConfig, GanttRenderRole, GanttSpec, build_gantt_render_rows,
};

use super::DocumentStyles;

pub(crate) const SEQUENCE_NOTE_LABEL_PREFIX: &str = "\x1fsequence-note:";

mod grid;
mod specs;

use grid::*;
pub use specs::*;

pub(crate) type StyledDiagramRows = Vec<Vec<Span>>;

#[derive(Clone, Copy, Debug)]
struct SequenceDiagramStyles {
    fill: Style,
    border: Style,
    label: Style,
    edge: Style,
    muted: Style,
}
pub(crate) fn rasterize_diagram(
    diagram: &ParsedDiagram,
    styles: &DocumentStyles,
) -> StyledDiagramRows {
    match diagram {
        ParsedDiagram::Flowchart(spec) => rasterize_flowchart(spec, styles),
        ParsedDiagram::Sequence(spec) => rasterize_sequence(spec, styles),
        ParsedDiagram::Class(spec) => rasterize_class(spec, styles),
        ParsedDiagram::State(spec) => rasterize_state(spec, styles),
        ParsedDiagram::Er(spec) => rasterize_er(spec, styles),
        ParsedDiagram::Pie(spec) => rasterize_pie(spec),
        ParsedDiagram::Gantt(spec) => rasterize_gantt(spec, styles),
    }
}

fn rasterize_flowchart(spec: &FlowchartSpec, styles: &DocumentStyles) -> StyledDiagramRows {
    let boxes = spec
        .nodes
        .iter()
        .map(|node| SimpleDiagramBox {
            id: node.id.clone(),
            rows: vec![node.label.clone()],
            divider_after: Vec::new(),
            fill_style: styles
                .diagram_node_fill_style
                .patch(node.style.fill_style()),
            border_style_fg: styles
                .diagram_node_border_style
                .patch(node.style.border_style()),
            label_style: styles
                .diagram_node_label_style
                .patch(node.style.label_style()),
            border_style: match node.shape {
                FlowNodeShape::Round | FlowNodeShape::Circle => BorderStyle::Rounded,
                _ => BorderStyle::Plain,
            },
            shape: match node.shape {
                FlowNodeShape::Cylinder => SimpleDiagramBoxShape::Cylinder,
                _ => SimpleDiagramBoxShape::Rect,
            },
        })
        .collect::<Vec<_>>();
    let edges = spec
        .edges
        .iter()
        .map(|edge| SimpleDiagramEdge {
            from: edge.from.clone(),
            to: edge.to.clone(),
            label: edge.label.clone(),
            from_label: None,
            to_label: None,
            line_style: styles.diagram_edge_style,
            label_style: styles.diagram_edge_style,
            dashed: edge.dashed,
            from_glyph: EndpointGlyph::None,
            to_glyph: EndpointGlyph::Arrow,
            prefer_vertical_backedge_labels: true,
        })
        .collect::<Vec<_>>();
    rasterize_simple_diagram_with_layer_gap(&boxes, &edges, 4)
}

fn rasterize_sequence(spec: &SequenceSpec, styles: &DocumentStyles) -> StyledDiagramRows {
    let participants = if spec.participants.is_empty() {
        let mut inferred: Vec<SequenceParticipantSpec> = Vec::new();
        for message in &spec.messages {
            for id in [&message.from, &message.to] {
                if !inferred.iter().any(|p| p.id.as_ref() == id.as_ref()) {
                    inferred.push(SequenceParticipantSpec {
                        id: id.clone(),
                        label: id.clone(),
                        actor: false,
                    });
                }
            }
        }
        inferred
    } else {
        spec.participants.clone()
    };
    if participants.is_empty() {
        return Vec::new();
    }

    let widths = participants
        .iter()
        .map(|p| unicode_width::UnicodeWidthStr::width(p.label.as_ref()).max(3) + 4)
        .collect::<Vec<_>>();
    let centers = widths
        .iter()
        .scan(0usize, |x, width| {
            let center = *x + width / 2;
            *x += *width + 4;
            Some(center)
        })
        .collect::<Vec<_>>();
    let total_width = sequence_total_width(spec, &participants, &centers, &widths);
    let message_y_base: usize = 4;
    let mut rows = styled_grid(
        total_width.max(1),
        message_y_base + spec.messages.len() * 2 + 1,
    );
    let sequence_styles = SequenceDiagramStyles {
        fill: styles.diagram_node_fill_style,
        border: styles
            .diagram_node_fill_style
            .patch(styles.diagram_node_border_style),
        label: styles
            .diagram_node_fill_style
            .patch(styles.diagram_node_label_style),
        edge: styles.diagram_edge_style,
        muted: styles.diagram_muted_style,
    };
    for (idx, participant) in participants.iter().enumerate() {
        draw_label_box(
            &mut rows,
            centers[idx],
            0,
            &participant.label,
            widths[idx],
            sequence_styles,
        );
    }
    for row_idx in 3..rows.len() {
        for center in &centers {
            put_char(&mut rows, *center, row_idx, '│', sequence_styles.muted);
        }
    }
    for (message_idx, message) in spec.messages.iter().enumerate() {
        let y = message_y_base + message_idx * 2;
        let from = participants
            .iter()
            .position(|p| p.id == message.from)
            .unwrap_or(0);
        let to = participants
            .iter()
            .position(|p| p.id == message.to)
            .unwrap_or(from);
        draw_sequence_message(
            &mut rows,
            centers[from],
            centers[to],
            y,
            message,
            sequence_styles,
        );
    }
    trim_styled_rows(rows)
}

fn rasterize_class(spec: &ClassSpec, styles: &DocumentStyles) -> StyledDiagramRows {
    let boxes = spec
        .classes
        .iter()
        .map(|class| {
            let mut rows = vec![class.name.clone()];
            let members = class
                .members
                .iter()
                .map(|m| {
                    let vis = match m.visibility {
                        ClassVisibilitySpec::Public => '+',
                        ClassVisibilitySpec::Private => '-',
                        ClassVisibilitySpec::Protected => '#',
                        ClassVisibilitySpec::Package => '~',
                    };
                    Arc::from(format!(
                        "{vis}{}{}",
                        m.name,
                        m.ty.as_deref()
                            .map(|t| format!(": {t}"))
                            .unwrap_or_default()
                    ))
                })
                .collect::<Vec<_>>();
            rows.extend(members);
            SimpleDiagramBox {
                id: class.name.clone(),
                rows,
                divider_after: vec![0],
                fill_style: styles.diagram_node_fill_style,
                border_style_fg: styles.diagram_node_border_style,
                label_style: styles.diagram_node_label_style,
                border_style: BorderStyle::Plain,
                shape: SimpleDiagramBoxShape::Rect,
            }
        })
        .collect::<Vec<_>>();
    let edges = spec
        .relations
        .iter()
        .map(|relation| {
            let (from_glyph, to_glyph, dashed) = class_relation_glyphs(&relation.arrow);
            SimpleDiagramEdge {
                from: relation.from.clone(),
                to: relation.to.clone(),
                label: relation.label.clone(),
                from_label: relation.from_cardinality.clone(),
                to_label: relation.to_cardinality.clone(),
                line_style: styles.diagram_edge_style,
                label_style: styles.diagram_edge_style,
                dashed,
                from_glyph,
                to_glyph,
                prefer_vertical_backedge_labels: false,
            }
        })
        .collect::<Vec<_>>();
    rasterize_simple_diagram(&boxes, &edges)
}

fn rasterize_state(spec: &StateSpec, styles: &DocumentStyles) -> StyledDiagramRows {
    let boxes = spec
        .states
        .iter()
        .map(|state| SimpleDiagramBox {
            id: state.id.clone(),
            rows: vec![Arc::from(match state.kind {
                StateKindSpec::Start | StateKindSpec::End => {
                    display_state_id(&state.id).to_string()
                }
                StateKindSpec::Choice => "◇".to_string(),
                StateKindSpec::State => state.label.to_string(),
            })],
            divider_after: Vec::new(),
            fill_style: styles.diagram_node_fill_style,
            border_style_fg: styles.diagram_node_border_style,
            label_style: styles.diagram_node_label_style,
            border_style: BorderStyle::Plain,
            shape: SimpleDiagramBoxShape::Rect,
        })
        .collect::<Vec<_>>();
    let edges = spec
        .transitions
        .iter()
        .map(|transition| SimpleDiagramEdge {
            from: transition.from.clone(),
            to: transition.to.clone(),
            label: transition.label.clone(),
            from_label: None,
            to_label: None,
            line_style: styles.diagram_edge_style,
            label_style: styles.diagram_edge_style,
            dashed: false,
            from_glyph: EndpointGlyph::None,
            to_glyph: EndpointGlyph::Arrow,
            prefer_vertical_backedge_labels: true,
        })
        .collect::<Vec<_>>();
    rasterize_simple_diagram(&boxes, &edges)
}

fn display_state_id(id: &str) -> &str {
    if id == "[*]$end" { "[*]" } else { id }
}

const SIMPLE_DIAGRAM_PADDING: Padding = Padding {
    top: 0,
    right: 1,
    bottom: 0,
    left: 1,
};

fn rasterize_simple_diagram(
    boxes: &[SimpleDiagramBox],
    edges: &[SimpleDiagramEdge],
) -> StyledDiagramRows {
    rasterize_simple_diagram_with_layer_gap(boxes, edges, 3)
}

fn rasterize_simple_diagram_with_layer_gap(
    boxes: &[SimpleDiagramBox],
    edges: &[SimpleDiagramEdge],
    layer_gap: u16,
) -> StyledDiagramRows {
    let (extra_layer, extra_node) = congestion_gap_extras(edges);
    let output = build_simple_diagram_output(
        boxes,
        edges,
        SIMPLE_DIAGRAM_PADDING,
        layer_gap.saturating_add(extra_layer),
        4u16.saturating_add(extra_node),
    );
    paint_simple_diagram(boxes, edges, &output)
}

// When a single node has many incoming or outgoing edges, the default lane
// budget runs out and arrows / labels collapse onto the same row. Grow the
// inter-layer and inter-node gaps proportionally so the router actually has
// distinct rows and columns to spread into. Capped to avoid pathological
// blow-up on degree-heavy hubs.
fn congestion_gap_extras(edges: &[SimpleDiagramEdge]) -> (u16, u16) {
    let mut incoming: HashMap<&str, u16> = HashMap::new();
    let mut outgoing: HashMap<&str, u16> = HashMap::new();
    for edge in edges {
        *incoming.entry(edge.to.as_ref()).or_default() += 1;
        *outgoing.entry(edge.from.as_ref()).or_default() += 1;
    }
    let max_in = incoming.values().copied().max().unwrap_or(0);
    let max_out = outgoing.values().copied().max().unwrap_or(0);
    const CAP: u16 = 6;
    let extra_layer = max_in.saturating_sub(2).min(CAP);
    let extra_node = max_out.saturating_sub(3).min(CAP);
    (extra_layer, extra_node)
}

fn paint_simple_diagram(
    boxes: &[SimpleDiagramBox],
    edges: &[SimpleDiagramEdge],
    output: &SimpleDiagramOutput,
) -> StyledDiagramRows {
    if output.width == 0 || output.height == 0 {
        return Vec::new();
    }
    let mut rows = styled_grid(output.width as usize, output.height as usize);
    let id_to_rect: HashMap<&str, Rect> = output
        .boxes
        .iter()
        .filter_map(|p| boxes.get(p.spec_index).map(|b| (b.id.as_ref(), p.rect)))
        .collect();
    // Merge cell bits across overlapping edges so junctions render as `┬`/`┼`/etc.
    // instead of one edge's corner glyph overpainting another. A cell is treated
    // as solid (not dashed) if any contributing edge is solid.
    let mut merged: HashMap<(i16, i16), (u8, bool, Style)> = HashMap::new();
    for edge in &output.edges {
        let Some(spec) = edges.get(edge.spec_index) else {
            continue;
        };
        for cell in &edge.cells {
            let entry = merged
                .entry((cell.x, cell.y))
                .or_insert((0, true, Style::default()));
            entry.0 |= cell.bits;
            entry.1 = entry.1 && spec.dashed;
            entry.2 = entry.2.patch(spec.line_style);
        }
    }
    for ((x, y), (bits, dashed, style)) in &merged {
        let ch = edge_glyph(*bits, *dashed);
        put_char_signed(&mut rows, *x, *y, ch, *style);
    }
    for positioned in &output.boxes {
        let Some(spec) = boxes.get(positioned.spec_index) else {
            continue;
        };
        let fill_style = spec.fill_style;
        let border_style = fill_style.patch(spec.border_style_fg);
        let label_style = fill_style.patch(spec.label_style);
        fill_rect_signed(&mut rows, positioned.rect, fill_style);
        draw_box_shape(
            &mut rows,
            positioned.rect,
            spec.border_style,
            spec.shape,
            border_style,
        );
        let content_x = positioned.rect.x + 1 + SIMPLE_DIAGRAM_PADDING.left as i16;
        let content_y = positioned.rect.y + 1 + SIMPLE_DIAGRAM_PADDING.top as i16;
        let dividers = normalized_dividers(&spec.divider_after, spec.rows.len());
        for (row_idx, row) in spec.rows.iter().enumerate() {
            let consumed_dividers = dividers
                .iter()
                .filter(|divider| **divider < row_idx)
                .count() as i16;
            put_text_signed(
                &mut rows,
                content_x,
                content_y
                    .saturating_add(row_idx as i16)
                    .saturating_add(consumed_dividers),
                row,
                label_style,
            );
        }
        for (divider_idx, divider) in dividers.iter().enumerate() {
            let y = content_y
                .saturating_add(*divider as i16)
                .saturating_add(1)
                .saturating_add(divider_idx as i16);
            if y > positioned.rect.y && y < positioned.rect.y + positioned.rect.h as i16 - 1 {
                for dx in 1..positioned.rect.w.saturating_sub(1) {
                    put_char_signed(
                        &mut rows,
                        positioned.rect.x + dx as i16,
                        y,
                        '─',
                        border_style,
                    );
                }
                put_char_signed(&mut rows, positioned.rect.x, y, '├', border_style);
                put_char_signed(
                    &mut rows,
                    positioned.rect.x + positioned.rect.w as i16 - 1,
                    y,
                    '┤',
                    border_style,
                );
            }
        }
    }
    for edge in &output.edges {
        let Some(spec) = edges.get(edge.spec_index) else {
            continue;
        };
        // Endpoint labels (cardinalities) sit next to box stems where flank
        // clearing prevents a `│1` collision. Edge-center labels live on the
        // trunk itself and read better INTEGRATED with the dashes
        // (`──contains──` instead of `── contains ──`), so we no longer clear
        // their flanks.
        if let (Some((x, y)), Some(label)) = (edge.from_label_pos, spec.from_label.as_ref()) {
            clear_label_flanks(&mut rows, x, y, label, spec.label_style);
            put_text_signed(&mut rows, x, y, label, spec.label_style);
        }
        if let (Some((x, y)), Some(label)) = (edge.to_label_pos, spec.to_label.as_ref()) {
            clear_label_flanks(&mut rows, x, y, label, spec.label_style);
            put_text_signed(&mut rows, x, y, label, spec.label_style);
        }
        if let (Some((x, y)), Some(label)) = (edge.label_pos, spec.label.as_ref()) {
            put_text_signed(&mut rows, x, y, label, spec.label_style);
        }
        if let Some((x, y)) = edge.from_pos {
            let (ex, ey, dir) = id_to_rect
                .get(spec.from.as_ref())
                .map(|rect| outside_endpoint(x, y, *rect))
                .unwrap_or((x, y, EndpointDir::Right));
            // If the outside cell is a fan-in/fan-out junction (channel runs
            // perpendicular to the edge approach), keep the junction glyph and
            // place the endpoint on the box border instead so the junction
            // topology stays readable.
            let (gx, gy) = if should_preserve_junction(spec.from_glyph)
                && cell_is_perpendicular_junction(&merged, ex, ey, dir)
            {
                (x, y)
            } else {
                (ex, ey)
            };
            put_endpoint(&mut rows, gx, gy, spec.from_glyph, dir, spec.line_style);
        }
        if let Some((x, y)) = edge.to_pos {
            let (ex, ey, dir) = id_to_rect
                .get(spec.to.as_ref())
                .map(|rect| outside_endpoint(x, y, *rect))
                .unwrap_or((x, y, EndpointDir::Right));
            let (gx, gy) = if should_preserve_junction(spec.to_glyph)
                && cell_is_perpendicular_junction(&merged, ex, ey, dir)
            {
                (x, y)
            } else {
                (ex, ey)
            };
            put_endpoint(&mut rows, gx, gy, spec.to_glyph, dir, spec.line_style);
        }
    }
    trim_styled_rows(rows)
}
fn sequence_total_width(
    spec: &SequenceSpec,
    participants: &[SequenceParticipantSpec],
    centers: &[usize],
    widths: &[usize],
) -> usize {
    let natural_width = widths.iter().sum::<usize>() + 4 * widths.len().saturating_sub(1);
    spec.messages
        .iter()
        .fold(natural_width, |max_width, message| {
            let from = participants
                .iter()
                .position(|p| p.id == message.from)
                .unwrap_or(0);
            let to = participants
                .iter()
                .position(|p| p.id == message.to)
                .unwrap_or(from);
            if let Some(note) = message.label.strip_prefix(SEQUENCE_NOTE_LABEL_PREFIX) {
                let center = (centers[from] + centers[to]) / 2;
                let width = sequence_note_width(note);
                max_width.max(center + width / 2 + 1)
            } else if from == to {
                max_width.max(
                    centers[from]
                        + 2
                        + unicode_width::UnicodeWidthStr::width(message.label.as_ref()),
                )
            } else {
                let start = centers[from].min(centers[to]);
                let end = centers[from].max(centers[to]);
                let label_width = unicode_width::UnicodeWidthStr::width(message.label.as_ref());
                let label_x = start + (end - start).saturating_sub(label_width) / 2;
                max_width.max(label_x + label_width)
            }
        })
}
fn rasterize_er(spec: &ErSpec, styles: &DocumentStyles) -> StyledDiagramRows {
    let boxes = spec
        .entities
        .iter()
        .map(|entity| {
            let mut rows = vec![entity.name.clone()];
            rows.extend(entity.attributes.iter().map(|a| {
                let keys = a
                    .keys
                    .iter()
                    .map(|k| k.as_ref())
                    .collect::<Vec<_>>()
                    .join(",");
                if keys.is_empty() {
                    Arc::from(format!("{} {}", a.ty, a.name))
                } else {
                    Arc::from(format!("{} {} {keys}", a.ty, a.name))
                }
            }));
            SimpleDiagramBox {
                id: entity.name.clone(),
                rows,
                divider_after: vec![0],
                fill_style: styles.diagram_node_fill_style,
                border_style_fg: styles.diagram_node_border_style,
                label_style: styles.diagram_node_label_style,
                border_style: BorderStyle::Plain,
                shape: SimpleDiagramBoxShape::Rect,
            }
        })
        .collect::<Vec<_>>();
    let edges = spec
        .relations
        .iter()
        .map(|relation| SimpleDiagramEdge {
            from: relation.left.clone(),
            to: relation.right.clone(),
            label: relation.label.clone(),
            from_label: None,
            to_label: None,
            line_style: styles.diagram_edge_style,
            label_style: styles.diagram_edge_style,
            dashed: false,
            from_glyph: er_cardinality_glyph(&relation.left_cardinality),
            to_glyph: er_cardinality_glyph(&relation.right_cardinality),
            prefer_vertical_backedge_labels: true,
        })
        .collect::<Vec<_>>();
    rasterize_simple_diagram(&boxes, &edges)
}

fn rasterize_pie(spec: &PieSpec) -> StyledDiagramRows {
    let mut rows = vec![spec.title.as_deref().unwrap_or("pie").to_string()];
    let total: f64 = spec.slices.iter().map(|s| s.value).sum();
    for slice in &spec.slices {
        let pct = if total > 0.0 {
            slice.value / total * 100.0
        } else {
            0.0
        };
        let bars = (pct / 5.0).round() as usize;
        rows.push(format!(
            "{:<16} {:>5.1}% {}",
            slice.label,
            pct,
            "█".repeat(bars)
        ));
    }
    plain_rows_to_spans(rows)
}

fn rasterize_gantt(spec: &GanttSpec, styles: &DocumentStyles) -> StyledDiagramRows {
    let rows = match build_gantt_render_rows(
        spec,
        GanttRenderConfig {
            max_timeline_width: 48,
        },
    ) {
        Ok(rows) => rows,
        Err(err) => return plain_rows_to_spans(vec![format!("gantt render error: {err}")]),
    };

    rows.rows
        .into_iter()
        .map(|row| {
            row.cells
                .into_iter()
                .enumerate()
                .flat_map(|(index, cell)| {
                    let mut spans = Vec::new();
                    if index > 0 {
                        spans.push(Span::new(" "));
                    }
                    spans.push(
                        Span::new(gantt_cell_text(cell.text.as_ref(), cell.role))
                            .style(gantt_role_style(cell.role, styles)),
                    );
                    spans
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn gantt_role_style(role: GanttRenderRole, styles: &DocumentStyles) -> Style {
    match role {
        GanttRenderRole::Title | GanttRenderRole::Section | GanttRenderRole::TaskLabel => {
            styles.diagram_node_label_style
        }
        GanttRenderRole::Axis => styles.diagram_muted_style,
        GanttRenderRole::PendingBar
        | GanttRenderRole::ActiveBar
        | GanttRenderRole::DoneBar
        | GanttRenderRole::CriticalBar
        | GanttRenderRole::Milestone => gantt_bar_style(role, styles),
        GanttRenderRole::Text => Style::default(),
    }
}

fn gantt_bar_style(role: GanttRenderRole, styles: &DocumentStyles) -> Style {
    let accent = style_fg(styles.diagram_node_border_style)
        .or_else(|| style_fg(styles.diagram_edge_style))
        .or_else(|| style_fg(styles.diagram_node_label_style))
        .unwrap_or(Color::LightBlue);
    let muted = style_fg(styles.diagram_muted_style).unwrap_or_else(|| accent.dim_by(0.45));
    let color = match role {
        GanttRenderRole::PendingBar => ColorGradient::new(muted, accent).color_at(0.62),
        GanttRenderRole::ActiveBar => accent,
        GanttRenderRole::DoneBar => ColorGradient::new(muted, accent).color_at(0.82),
        GanttRenderRole::CriticalBar => accent.lighten_by(0.18),
        GanttRenderRole::Milestone => accent.lighten_by(0.28),
        _ => accent,
    };
    let mut style = Style::new().fg(color);
    if matches!(
        role,
        GanttRenderRole::CriticalBar | GanttRenderRole::Milestone
    ) {
        style = style.bold();
    }
    style
}

fn gantt_cell_text(text: &str, role: GanttRenderRole) -> String {
    match role {
        GanttRenderRole::PendingBar
        | GanttRenderRole::ActiveBar
        | GanttRenderRole::DoneBar
        | GanttRenderRole::CriticalBar => text
            .chars()
            .map(|ch| if ch == ' ' { ' ' } else { '█' })
            .collect(),
        _ => text.to_owned(),
    }
}

fn style_fg(style: Style) -> Option<Color> {
    style.fg.map(|paint| paint.color()).and_then(usable_color)
}

fn usable_color(color: Color) -> Option<Color> {
    (!matches!(color, Color::Reset | Color::Backdrop | Color::Transparent)).then_some(color)
}

#[cfg(test)]
mod tests;
