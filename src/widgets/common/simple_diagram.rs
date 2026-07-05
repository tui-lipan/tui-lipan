use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use unicode_width::UnicodeWidthStr;

use crate::layout::dag::{DagEdge, DagInput, DagLayoutOptions, DagNode, DagPoint, DagSize};
use crate::style::{BorderStyle, Padding, Rect, Style};
use crate::widgets::common::box_glyphs::{ALL_DIRECTIONS, EAST, NORTH, SOUTH, WEST};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum EndpointGlyph {
    None,
    Arrow,
    Triangle,
    Diamond,
    Circle,
    CrowZeroOrOne,
    CrowExactlyOne,
    CrowZeroOrMore,
    CrowOneOrMore,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum SimpleDiagramBoxShape {
    #[default]
    Rect,
    Cylinder,
}

#[derive(Clone, Debug)]
pub(crate) struct SimpleDiagramBox {
    pub(crate) id: Arc<str>,
    pub(crate) rows: Vec<Arc<str>>,
    pub(crate) divider_after: Vec<usize>,
    pub(crate) fill_style: Style,
    pub(crate) border_style_fg: Style,
    pub(crate) label_style: Style,
    pub(crate) border_style: BorderStyle,
    pub(crate) shape: SimpleDiagramBoxShape,
}

impl SimpleDiagramBox {
    pub(crate) fn min_size(&self, padding: Padding) -> DagSize {
        let divider_count = normalized_dividers(&self.divider_after, self.rows.len()).len() as u16;
        let width = self
            .rows
            .iter()
            .map(|row| UnicodeWidthStr::width(row.as_ref()).min(u16::MAX as usize) as u16)
            .max()
            .unwrap_or(0)
            .saturating_add(padding.horizontal())
            .saturating_add(2)
            .max(3);
        let width = if width % 2 == 0 {
            width.saturating_add(1)
        } else {
            width
        };
        let height = (self.rows.len() as u16)
            .saturating_add(divider_count)
            .saturating_add(padding.vertical())
            .saturating_add(2)
            .max(3);
        DagSize::new(width, height)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SimpleDiagramEdge {
    pub(crate) from: Arc<str>,
    pub(crate) to: Arc<str>,
    pub(crate) label: Option<Arc<str>>,
    pub(crate) from_label: Option<Arc<str>>,
    pub(crate) to_label: Option<Arc<str>>,
    pub(crate) line_style: Style,
    pub(crate) label_style: Style,
    pub(crate) dashed: bool,
    pub(crate) from_glyph: EndpointGlyph,
    pub(crate) to_glyph: EndpointGlyph,
    pub(crate) prefer_vertical_backedge_labels: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SimplePositionedBox {
    pub(crate) spec_index: usize,
    pub(crate) rect: Rect,
}

#[derive(Clone, Debug)]
pub(crate) struct SimpleEdgeCell {
    pub(crate) x: i16,
    pub(crate) y: i16,
    pub(crate) bits: u8,
}

#[derive(Clone, Debug)]
pub(crate) struct SimplePositionedEdge {
    pub(crate) spec_index: usize,
    pub(crate) cells: Vec<SimpleEdgeCell>,
    pub(crate) label_pos: Option<(i16, i16)>,
    pub(crate) from_label_pos: Option<(i16, i16)>,
    pub(crate) to_label_pos: Option<(i16, i16)>,
    pub(crate) from_pos: Option<(i16, i16)>,
    pub(crate) to_pos: Option<(i16, i16)>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SimpleDiagramOutput {
    pub(crate) boxes: Vec<SimplePositionedBox>,
    pub(crate) edges: Vec<SimplePositionedEdge>,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

#[derive(Clone, Debug)]
struct RoutedSimpleEdge {
    spec_index: usize,
    cells: Vec<SimpleEdgeCell>,
    points: Vec<DagPoint>,
    reversed: bool,
    from_pos: Option<(i16, i16)>,
    to_pos: Option<(i16, i16)>,
}

#[derive(Clone, Copy, Debug, Default)]
struct EdgeTextPositions {
    label_pos: Option<(i16, i16)>,
    from_label_pos: Option<(i16, i16)>,
    to_label_pos: Option<(i16, i16)>,
}

pub(crate) fn build_simple_diagram_output(
    boxes: &[SimpleDiagramBox],
    edges: &[SimpleDiagramEdge],
    padding: Padding,
    layer_gap: u16,
    node_gap: u16,
) -> SimpleDiagramOutput {
    if boxes.is_empty() {
        return SimpleDiagramOutput::default();
    }
    let dag_nodes = boxes
        .iter()
        .map(|node| DagNode::new(node.id.clone(), node.min_size(padding)))
        .collect::<Vec<_>>();
    let dag_edges = edges
        .iter()
        .map(|edge| {
            let mut dag_edge = DagEdge::new(edge.from.clone(), edge.to.clone()).heads(
                !matches!(edge.from_glyph, EndpointGlyph::None),
                !matches!(edge.to_glyph, EndpointGlyph::None),
            );
            if let Some(label) = &edge.label {
                dag_edge = dag_edge.label(label.clone());
            }
            dag_edge
        })
        .collect::<Vec<_>>();
    let mut input = DagInput::new(dag_nodes, dag_edges);
    input.options = DagLayoutOptions {
        layer_gap,
        node_gap,
        margin_x: 1,
        margin_y: 1,
    };
    let output = crate::layout::dag::compute(input);
    let id_to_box = boxes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), index))
        .collect::<HashMap<_, _>>();
    let positioned_boxes = output
        .positioned_nodes
        .into_iter()
        .filter_map(|node| {
            id_to_box
                .get(&node.id)
                .copied()
                .map(|spec_index| SimplePositionedBox {
                    spec_index,
                    rect: node.rect,
                })
        })
        .collect::<Vec<_>>();
    let routed_edges = output
        .routed_edges
        .into_iter()
        .filter_map(|edge| {
            let spec_index = edges
                .iter()
                .position(|candidate| candidate.from == edge.from && candidate.to == edge.to)?;
            let points = edge.points;
            Some(RoutedSimpleEdge {
                spec_index,
                cells: cells_from_points(&points),
                from_pos: points.first().map(|p| (p.x, p.y)),
                to_pos: points.last().map(|p| (p.x, p.y)),
                points,
                reversed: edge.reversed,
            })
        })
        .collect::<Vec<_>>();
    let text_positions = place_edge_labels(&routed_edges, boxes, edges, &positioned_boxes);
    let mut positioned_edges = routed_edges
        .into_iter()
        .zip(text_positions)
        .map(|(edge, text)| SimplePositionedEdge {
            spec_index: edge.spec_index,
            cells: edge.cells,
            label_pos: text.label_pos,
            from_label_pos: text.from_label_pos,
            to_label_pos: text.to_label_pos,
            from_pos: edge.from_pos,
            to_pos: edge.to_pos,
        })
        .collect::<Vec<_>>();
    let mut positioned_boxes = positioned_boxes;
    let bounds = output_bounds(output.bounds, &positioned_boxes, &positioned_edges, edges);
    normalize_output_positions(bounds, &mut positioned_boxes, &mut positioned_edges);
    SimpleDiagramOutput {
        boxes: positioned_boxes,
        edges: positioned_edges,
        width: bounds.w,
        height: bounds.h,
    }
}

pub(crate) fn normalized_dividers(divider_after: &[usize], row_count: usize) -> Vec<usize> {
    let mut dividers = divider_after
        .iter()
        .copied()
        .filter(|divider| *divider < row_count)
        .collect::<Vec<_>>();
    dividers.sort_unstable();
    dividers.dedup();
    dividers
}

fn cells_from_points(points: &[DagPoint]) -> Vec<SimpleEdgeCell> {
    let mut bits: HashMap<(i16, i16), u8> = HashMap::new();
    for pair in points.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if a.x == b.x {
            let (start, end) = if a.y <= b.y { (a.y, b.y) } else { (b.y, a.y) };
            for y in start..=end {
                let entry = bits.entry((a.x, y)).or_default();
                if y > start {
                    *entry |= NORTH;
                }
                if y < end {
                    *entry |= SOUTH;
                }
            }
        } else if a.y == b.y {
            let (start, end) = if a.x <= b.x { (a.x, b.x) } else { (b.x, a.x) };
            for x in start..=end {
                let entry = bits.entry((x, a.y)).or_default();
                if x > start {
                    *entry |= WEST;
                }
                if x < end {
                    *entry |= EAST;
                }
            }
        }
    }
    bits.into_iter()
        .filter_map(|((x, y), bits)| {
            let bits = bits & ALL_DIRECTIONS;
            (bits != 0).then_some(SimpleEdgeCell { x, y, bits })
        })
        .collect()
}

fn place_edge_labels(
    routed_edges: &[RoutedSimpleEdge],
    boxes: &[SimpleDiagramBox],
    edges: &[SimpleDiagramEdge],
    positioned_boxes: &[SimplePositionedBox],
) -> Vec<EdgeTextPositions> {
    let id_to_rect = positioned_boxes
        .iter()
        .filter_map(|positioned| {
            boxes
                .get(positioned.spec_index)
                .map(|spec| (spec.id.as_ref(), positioned.rect))
        })
        .collect::<HashMap<_, _>>();
    let mut occupied = HashSet::new();
    for rect in id_to_rect.values().copied() {
        mark_rect_cells(rect, &mut occupied);
    }
    for edge in routed_edges {
        let Some(spec) = edges.get(edge.spec_index) else {
            continue;
        };
        mark_endpoint_blockers(
            edge.from_pos,
            id_to_rect.get(spec.from.as_ref()).copied(),
            &mut occupied,
        );
        mark_endpoint_blockers(
            edge.to_pos,
            id_to_rect.get(spec.to.as_ref()).copied(),
            &mut occupied,
        );
    }
    let route_cells = routed_edges
        .iter()
        .flat_map(|edge| edge.cells.iter().map(|cell| (cell.x, cell.y)))
        .collect::<HashSet<_>>();

    let mut positions = Vec::with_capacity(routed_edges.len());
    for edge in routed_edges {
        let Some(spec) = edges.get(edge.spec_index) else {
            positions.push(EdgeTextPositions::default());
            continue;
        };

        let from_label_pos = spec.from_label.as_ref().and_then(|label| {
            endpoint_label_position(
                &edge.points,
                edge.from_pos,
                id_to_rect.get(spec.from.as_ref()).copied(),
                label,
                true,
                &occupied,
            )
        });
        if let (Some((x, y)), Some(label)) = (from_label_pos, spec.from_label.as_ref()) {
            mark_label_cells(x, y, label, &mut occupied);
        }

        let to_label_pos = spec.to_label.as_ref().and_then(|label| {
            endpoint_label_position(
                &edge.points,
                edge.to_pos,
                id_to_rect.get(spec.to.as_ref()).copied(),
                label,
                false,
                &occupied,
            )
        });
        if let (Some((x, y)), Some(label)) = (to_label_pos, spec.to_label.as_ref()) {
            mark_label_cells(x, y, label, &mut occupied);
        }

        let label_pos = spec.label.as_ref().and_then(|label| {
            label_position(
                &edge.points,
                edge.reversed && spec.prefer_vertical_backedge_labels,
                label,
                &occupied,
                &route_cells,
            )
        });
        if let (Some((x, y)), Some(label)) = (label_pos, spec.label.as_ref()) {
            mark_label_cells(x, y, label, &mut occupied);
        }

        positions.push(EdgeTextPositions {
            label_pos,
            from_label_pos,
            to_label_pos,
        });
    }
    positions
}

fn label_position(
    points: &[DagPoint],
    prefer_vertical: bool,
    label: &str,
    occupied: &HashSet<(i16, i16)>,
    route_cells: &HashSet<(i16, i16)>,
) -> Option<(i16, i16)> {
    let label_width = UnicodeWidthStr::width(label).min(i16::MAX as usize) as i16;
    let slot_width = label_slot_width(label_width);
    let segments = points
        .windows(2)
        .filter_map(|pair| LabelSegment::new(pair[0], pair[1]))
        .collect::<Vec<_>>();
    let mut preferred = segments
        .iter()
        .copied()
        .filter(|segment| segment.is_vertical() == prefer_vertical)
        .collect::<Vec<_>>();
    let mut fallback = segments
        .iter()
        .copied()
        .filter(|segment| segment.is_vertical() != prefer_vertical)
        .collect::<Vec<_>>();
    preferred.sort_by(|a, b| {
        b.length
            .cmp(&a.length)
            .then_with(|| a.start.y.cmp(&b.start.y))
            .then_with(|| a.start.x.cmp(&b.start.x))
    });
    fallback.sort_by(|a, b| {
        b.length
            .cmp(&a.length)
            .then_with(|| a.start.y.cmp(&b.start.y))
            .then_with(|| a.start.x.cmp(&b.start.x))
    });

    for segment in preferred.iter().chain(fallback.iter()).copied() {
        if let Some(position) = segment_label_position(segment, slot_width, occupied, route_cells) {
            return Some(position);
        }
    }

    // Slot search failed on every segment — likely a sibling edge already
    // claimed the natural midpoint. Try shifting the label one row above or
    // below the segment (still visually attached to the edge) before giving
    // up.
    for segment in preferred.iter().chain(fallback.iter()).copied() {
        if let Some(position) = segment_label_position_off_axis(segment, slot_width, occupied) {
            return Some(position);
        }
    }

    let segment = preferred
        .first()
        .copied()
        .or_else(|| fallback.first().copied())?;
    if segment.is_vertical() {
        Some((
            segment.start.x.saturating_add(2),
            signed_midpoint(segment.start.y, segment.end.y),
        ))
    } else {
        let slot_x = signed_midpoint(segment.start.x, segment.end.x).saturating_sub(slot_width / 2);
        Some((slot_x.saturating_add(1), segment.start.y))
    }
}

fn segment_label_position_off_axis(
    segment: LabelSegment,
    slot_width: i16,
    occupied: &HashSet<(i16, i16)>,
) -> Option<(i16, i16)> {
    if segment.is_vertical() {
        // Vertical segment: try shifting the label one column further from
        // the channel.
        let right_slot_x = segment.start.x.saturating_add(2);
        let left_slot_x = segment.start.x.saturating_sub(slot_width).saturating_sub(1);
        for label_y in axis_values_on_segment(segment.start.y, segment.end.y) {
            for slot_x in [right_slot_x, left_slot_x] {
                if label_slot_free(slot_x, label_y, slot_width, occupied) {
                    return Some((slot_x.saturating_add(1), label_y));
                }
            }
        }
    } else {
        // Horizontal segment: try one row above or below, scanning the same
        // x range as the segment.
        let (lo, hi) = if segment.start.x <= segment.end.x {
            (segment.start.x, segment.end.x)
        } else {
            (segment.end.x, segment.start.x)
        };
        for label_y in [
            segment.start.y.saturating_sub(1),
            segment.start.y.saturating_add(1),
        ] {
            for center_x in axis_values_on_segment(segment.start.x, segment.end.x) {
                let slot_x = center_x.saturating_sub(slot_width / 2);
                let slot_end = slot_x.saturating_add(slot_width).saturating_sub(1);
                if slot_x >= lo
                    && slot_end <= hi
                    && label_slot_free(slot_x, label_y, slot_width, occupied)
                {
                    return Some((slot_x.saturating_add(1), label_y));
                }
            }
        }
    }
    None
}

fn segment_label_position(
    segment: LabelSegment,
    slot_width: i16,
    occupied: &HashSet<(i16, i16)>,
    route_cells: &HashSet<(i16, i16)>,
) -> Option<(i16, i16)> {
    if segment.is_vertical() {
        let right_slot_x = segment.start.x.saturating_add(1);
        let left_slot_x = segment.start.x.saturating_sub(slot_width);
        for label_y in axis_values_on_segment(segment.start.y, segment.end.y) {
            let mut candidates = Vec::new();
            if label_slot_free(left_slot_x, label_y, slot_width, occupied)
                && label_slot_free(left_slot_x, label_y, slot_width, route_cells)
            {
                candidates.push((
                    label_side_clearance(
                        left_slot_x,
                        label_y,
                        slot_width,
                        -1,
                        occupied,
                        route_cells,
                    ),
                    0,
                    left_slot_x,
                ));
            }
            if label_slot_free(right_slot_x, label_y, slot_width, occupied)
                && label_slot_free(right_slot_x, label_y, slot_width, route_cells)
            {
                candidates.push((
                    label_side_clearance(
                        right_slot_x,
                        label_y,
                        slot_width,
                        1,
                        occupied,
                        route_cells,
                    ),
                    1,
                    right_slot_x,
                ));
            }
            if let Some((_, _, slot_x)) = candidates.into_iter().max_by_key(|candidate| {
                // Prefer the side with more open space before nearby lines or labels.
                // If both sides are equivalent, keep the historical right-side bias.
                (candidate.0, candidate.1)
            }) {
                return Some((slot_x.saturating_add(1), label_y));
            }
        }
    } else {
        let (lo, hi) = if segment.start.x <= segment.end.x {
            (segment.start.x, segment.end.x)
        } else {
            (segment.end.x, segment.start.x)
        };
        for center_x in axis_values_on_segment(segment.start.x, segment.end.x) {
            let slot_x = center_x.saturating_sub(slot_width / 2);
            let slot_end = slot_x.saturating_add(slot_width).saturating_sub(1);
            if slot_x >= lo
                && slot_end <= hi
                && label_slot_free(slot_x, segment.start.y, slot_width, occupied)
            {
                return Some((slot_x.saturating_add(1), segment.start.y));
            }
        }
    }
    None
}

fn label_side_clearance(
    slot_x: i16,
    y: i16,
    slot_width: i16,
    direction: i16,
    occupied: &HashSet<(i16, i16)>,
    route_cells: &HashSet<(i16, i16)>,
) -> i16 {
    let mut x = if direction < 0 {
        slot_x.saturating_sub(1)
    } else {
        slot_x.saturating_add(slot_width)
    };
    let mut clearance = 0i16;
    for _ in 0..24 {
        if x < 0 {
            return 24;
        }
        if occupied.contains(&(x, y)) || route_cells.contains(&(x, y)) {
            break;
        }
        clearance = clearance.saturating_add(1);
        x = x.saturating_add(direction);
    }
    clearance
}

fn endpoint_label_position(
    points: &[DagPoint],
    point: Option<(i16, i16)>,
    rect: Option<Rect>,
    label: &str,
    is_source: bool,
    occupied: &HashSet<(i16, i16)>,
) -> Option<(i16, i16)> {
    let (x, y) = point?;
    let rect = rect?;
    let (outside_x, outside_y, side) = outside_endpoint_side(x, y, rect);
    let label_width = UnicodeWidthStr::width(label).min(i16::MAX as usize) as i16;
    let slot_width = label_slot_width(label_width);
    let segment_anchor = if is_source {
        points
            .get(1)
            .copied()
            .unwrap_or(DagPoint::new(outside_x, outside_y))
    } else {
        points
            .get(points.len().saturating_sub(2))
            .copied()
            .unwrap_or(DagPoint::new(outside_x, outside_y))
    };

    match side {
        EndpointSide::North | EndpointSide::South => {
            // Skip the 1-cell endpoint halo so the cardinality label can sit on
            // the natural row next to the endpoint glyph instead of being pushed
            // away or dropped entirely.
            let left_slot_x = outside_x.saturating_sub(slot_width).saturating_sub(1);
            let right_slot_x = outside_x.saturating_add(2);
            let candidates = if is_source {
                [left_slot_x, right_slot_x]
            } else {
                [right_slot_x, left_slot_x]
            };
            for slot_y in axis_values_from_endpoint(outside_y, segment_anchor.y, side) {
                for slot_x in candidates {
                    if label_slot_free(slot_x, slot_y, slot_width, occupied) {
                        return Some((slot_x.saturating_add(1), slot_y));
                    }
                }
            }
        }
        EndpointSide::East | EndpointSide::West => {
            let above_y = outside_y.saturating_sub(1);
            let below_y = outside_y.saturating_add(1);
            let candidates = if is_source {
                [above_y, below_y]
            } else {
                [below_y, above_y]
            };
            for anchor_x in axis_values_from_endpoint(outside_x, segment_anchor.x, side) {
                let label_x = anchor_x.saturating_sub(label_width / 2);
                let slot_x = label_x.saturating_sub(1);
                for slot_y in candidates {
                    if label_slot_free(slot_x, slot_y, slot_width, occupied) {
                        return Some((label_x, slot_y));
                    }
                }
            }
        }
    }

    None
}

fn axis_values_on_segment(a: i16, b: i16) -> Vec<i16> {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let midpoint = signed_midpoint(a, b);
    let mut values = (lo..=hi).collect::<Vec<_>>();
    values.sort_by_key(|value| {
        (
            i32::from(value.saturating_sub(midpoint).abs()),
            i32::from(value.saturating_sub(lo).abs()),
            *value,
        )
    });
    values
}

fn axis_values_from_endpoint(start: i16, end: i16, side: EndpointSide) -> Vec<i16> {
    let mut values = Vec::new();
    values.push(start);
    match side {
        EndpointSide::North => {
            for value in ((end.min(start))..start).rev() {
                if value != start {
                    values.push(value);
                }
            }
        }
        EndpointSide::South => {
            for value in start.saturating_add(1)..=end.max(start) {
                values.push(value);
            }
        }
        EndpointSide::West => {
            for value in ((end.min(start))..start).rev() {
                if value != start {
                    values.push(value);
                }
            }
        }
        EndpointSide::East => {
            for value in start.saturating_add(1)..=end.max(start) {
                values.push(value);
            }
        }
    }
    values.dedup();
    values
}

fn label_slot_width(label_width: i16) -> i16 {
    label_width.saturating_add(2)
}

fn label_slot_free(x: i16, y: i16, slot_width: i16, occupied: &HashSet<(i16, i16)>) -> bool {
    if x < 0 || y < 0 || slot_width <= 0 {
        return false;
    }
    (0..slot_width).all(|offset| !occupied.contains(&(x.saturating_add(offset), y)))
}

fn mark_rect_cells(rect: Rect, occupied: &mut HashSet<(i16, i16)>) {
    for y in rect.y..rect.y.saturating_add(rect.h as i16) {
        for x in rect.x..rect.x.saturating_add(rect.w as i16) {
            occupied.insert((x, y));
        }
    }
}

fn mark_label_cells(x: i16, y: i16, label: &str, occupied: &mut HashSet<(i16, i16)>) {
    let label_width = UnicodeWidthStr::width(label).min(i16::MAX as usize) as i16;
    let slot_x = x.saturating_sub(1);
    let slot_width = label_slot_width(label_width);
    for offset in 0..slot_width {
        occupied.insert((slot_x.saturating_add(offset), y));
    }
}

fn mark_endpoint_blockers(
    point: Option<(i16, i16)>,
    rect: Option<Rect>,
    occupied: &mut HashSet<(i16, i16)>,
) {
    let Some((x, y)) = point else {
        return;
    };
    occupied.insert((x, y));
    if let Some(rect) = rect {
        let (outside_x, outside_y, side) = outside_endpoint_side(x, y, rect);
        occupied.insert((outside_x, outside_y));
        match side {
            EndpointSide::North | EndpointSide::South => {
                occupied.insert((outside_x.saturating_sub(1), outside_y));
                occupied.insert((outside_x.saturating_add(1), outside_y));
            }
            EndpointSide::East | EndpointSide::West => {
                occupied.insert((outside_x, outside_y.saturating_sub(1)));
                occupied.insert((outside_x, outside_y.saturating_add(1)));
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EndpointSide {
    North,
    South,
    East,
    West,
}

fn outside_endpoint_side(x: i16, y: i16, rect: Rect) -> (i16, i16, EndpointSide) {
    let right = rect.x.saturating_add(rect.w as i16).saturating_sub(1);
    let bottom = rect.y.saturating_add(rect.h as i16).saturating_sub(1);
    if y <= rect.y {
        (x, y.saturating_sub(1), EndpointSide::North)
    } else if y >= bottom {
        (x, y.saturating_add(1), EndpointSide::South)
    } else if x <= rect.x {
        (x.saturating_sub(1), y, EndpointSide::West)
    } else if x >= right {
        (x.saturating_add(1), y, EndpointSide::East)
    } else {
        (x, y, EndpointSide::East)
    }
}

#[cfg(test)]
fn is_rect_border_cell(rect: Rect, x: i16, y: i16) -> bool {
    let right = rect.x.saturating_add(rect.w as i16).saturating_sub(1);
    let bottom = rect.y.saturating_add(rect.h as i16).saturating_sub(1);
    x == rect.x || x == right || y == rect.y || y == bottom
}

fn normalize_output_positions(
    bounds: Rect,
    positioned_boxes: &mut [SimplePositionedBox],
    positioned_edges: &mut [SimplePositionedEdge],
) {
    if bounds.x >= 0 && bounds.y >= 0 {
        return;
    }

    let shift_x = 0i16.saturating_sub(bounds.x.min(0));
    let shift_y = 0i16.saturating_sub(bounds.y.min(0));
    for node in positioned_boxes {
        node.rect.x = node.rect.x.saturating_add(shift_x);
        node.rect.y = node.rect.y.saturating_add(shift_y);
    }
    for edge in positioned_edges {
        for cell in &mut edge.cells {
            cell.x = cell.x.saturating_add(shift_x);
            cell.y = cell.y.saturating_add(shift_y);
        }
        if let Some((x, y)) = &mut edge.label_pos {
            *x = x.saturating_add(shift_x);
            *y = y.saturating_add(shift_y);
        }
        if let Some((x, y)) = &mut edge.from_label_pos {
            *x = x.saturating_add(shift_x);
            *y = y.saturating_add(shift_y);
        }
        if let Some((x, y)) = &mut edge.to_label_pos {
            *x = x.saturating_add(shift_x);
            *y = y.saturating_add(shift_y);
        }
        if let Some((x, y)) = &mut edge.from_pos {
            *x = x.saturating_add(shift_x);
            *y = y.saturating_add(shift_y);
        }
        if let Some((x, y)) = &mut edge.to_pos {
            *x = x.saturating_add(shift_x);
            *y = y.saturating_add(shift_y);
        }
    }
}

#[derive(Clone, Copy)]
struct LabelSegment {
    start: DagPoint,
    end: DagPoint,
    length: i32,
}

impl LabelSegment {
    fn new(start: DagPoint, end: DagPoint) -> Option<Self> {
        let length = (i32::from(start.x) - i32::from(end.x)).abs()
            + (i32::from(start.y) - i32::from(end.y)).abs();
        (length > 0).then_some(Self { start, end, length })
    }

    fn is_vertical(&self) -> bool {
        self.start.x == self.end.x
    }
}

fn signed_midpoint(a: i16, b: i16) -> i16 {
    ((i32::from(a) + i32::from(b)) / 2).clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

fn output_bounds(
    mut bounds: Rect,
    positioned_boxes: &[SimplePositionedBox],
    positioned_edges: &[SimplePositionedEdge],
    edges: &[SimpleDiagramEdge],
) -> Rect {
    let mut min_x = bounds.x;
    let mut min_y = bounds.y;
    let mut max_x = bounds.x.saturating_add(bounds.w as i16);
    let mut max_y = bounds.y.saturating_add(bounds.h as i16);

    for node in positioned_boxes {
        let rect = node.rect;
        min_x = min_x.min(rect.x);
        min_y = min_y.min(rect.y);
        max_x = max_x.max(rect.x.saturating_add(rect.w as i16));
        max_y = max_y.max(rect.y.saturating_add(rect.h as i16));
    }
    for edge in positioned_edges {
        for cell in &edge.cells {
            min_x = min_x.min(cell.x);
            min_y = min_y.min(cell.y);
            max_x = max_x.max(cell.x.saturating_add(1));
            max_y = max_y.max(cell.y.saturating_add(1));
        }
        if let (Some((x, y)), Some(label)) = (
            edge.label_pos,
            edges
                .get(edge.spec_index)
                .and_then(|spec| spec.label.as_ref()),
        ) {
            min_x = min_x.min(x.saturating_sub(1));
            min_y = min_y.min(y);
            let label_width = UnicodeWidthStr::width(label.as_ref()).min(i16::MAX as usize) as i16;
            max_x = max_x.max(x.saturating_add(label_width).saturating_add(1));
            max_y = max_y.max(y.saturating_add(1));
        }
        if let (Some((x, y)), Some(label)) = (
            edge.from_label_pos,
            edges
                .get(edge.spec_index)
                .and_then(|spec| spec.from_label.as_ref()),
        ) {
            min_x = min_x.min(x.saturating_sub(1));
            min_y = min_y.min(y);
            let label_width = UnicodeWidthStr::width(label.as_ref()).min(i16::MAX as usize) as i16;
            max_x = max_x.max(x.saturating_add(label_width).saturating_add(1));
            max_y = max_y.max(y.saturating_add(1));
        }
        if let (Some((x, y)), Some(label)) = (
            edge.to_label_pos,
            edges
                .get(edge.spec_index)
                .and_then(|spec| spec.to_label.as_ref()),
        ) {
            min_x = min_x.min(x.saturating_sub(1));
            min_y = min_y.min(y);
            let label_width = UnicodeWidthStr::width(label.as_ref()).min(i16::MAX as usize) as i16;
            max_x = max_x.max(x.saturating_add(label_width).saturating_add(1));
            max_y = max_y.max(y.saturating_add(1));
        }
        if let Some((x, y)) = edge.from_pos {
            if let Some(spec) = edges.get(edge.spec_index) {
                let width = endpoint_width(spec.from_glyph);
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x.saturating_add(width as i16));
                max_y = max_y.max(y.saturating_add(1));
            }
        }
        if let Some((x, y)) = edge.to_pos {
            if let Some(spec) = edges.get(edge.spec_index) {
                let width = endpoint_width(spec.to_glyph);
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x.saturating_add(width as i16));
                max_y = max_y.max(y.saturating_add(1));
            }
        }
    }

    bounds.x = min_x;
    bounds.y = min_y;
    bounds.w = (max_x - min_x).max(0) as u16;
    bounds.h = (max_y - min_y).max(0) as u16;
    bounds
}

fn endpoint_width(glyph: EndpointGlyph) -> u16 {
    match glyph {
        EndpointGlyph::None => 0,
        EndpointGlyph::CrowZeroOrOne
        | EndpointGlyph::CrowExactlyOne
        | EndpointGlyph::CrowZeroOrMore
        | EndpointGlyph::CrowOneOrMore => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagram_box(id: &str, rows: &[&str], divider_after: Vec<usize>) -> SimpleDiagramBox {
        SimpleDiagramBox {
            id: id.into(),
            rows: rows.iter().map(|row| Arc::<str>::from(*row)).collect(),
            divider_after,
            fill_style: Style::default(),
            border_style_fg: Style::default(),
            label_style: Style::default(),
            border_style: BorderStyle::Plain,
            shape: SimpleDiagramBoxShape::Rect,
        }
    }

    #[test]
    fn dividers_consume_box_rows() {
        let box_spec = diagram_box("User", &["User", "+id: i32", "+save(): bool"], vec![0, 1]);

        assert_eq!(box_spec.min_size(Padding::default()).h, 7);
    }

    #[test]
    fn edge_label_bounds_extend_output_width() {
        let boxes = vec![
            diagram_box("A", &["A"], Vec::new()),
            diagram_box("B", &["B"], Vec::new()),
        ];
        let edges = vec![SimpleDiagramEdge {
            from: "A".into(),
            to: "B".into(),
            label: Some("very long transition label".into()),
            from_label: None,
            to_label: None,
            line_style: Style::default(),
            label_style: Style::default(),
            dashed: false,
            from_glyph: EndpointGlyph::None,
            to_glyph: EndpointGlyph::Arrow,
            prefer_vertical_backedge_labels: true,
        }];

        let output = build_simple_diagram_output(&boxes, &edges, Padding::default(), 1, 1);
        let edge = &output.edges[0];
        let (label_x, _) = edge.label_pos.unwrap();
        let label_width = UnicodeWidthStr::width(edges[0].label.as_ref().unwrap().as_ref()) as i16;

        assert!(label_x.saturating_add(label_width) <= output.width as i16);
        assert!(label_x > edge.cells.iter().map(|cell| cell.x).min().unwrap());
    }

    #[test]
    fn edge_label_stays_between_vertically_stacked_nodes() {
        let boxes = vec![
            diagram_box("A", &["A"], Vec::new()),
            diagram_box("B", &["B"], Vec::new()),
        ];
        let edges = vec![SimpleDiagramEdge {
            from: "A".into(),
            to: "B".into(),
            label: Some("go".into()),
            from_label: None,
            to_label: None,
            line_style: Style::default(),
            label_style: Style::default(),
            dashed: false,
            from_glyph: EndpointGlyph::None,
            to_glyph: EndpointGlyph::Arrow,
            prefer_vertical_backedge_labels: true,
        }];

        let output = build_simple_diagram_output(&boxes, &edges, Padding::default(), 1, 1);
        let from_rect = output
            .boxes
            .iter()
            .find(|node| node.spec_index == 0)
            .unwrap()
            .rect;
        let to_rect = output
            .boxes
            .iter()
            .find(|node| node.spec_index == 1)
            .unwrap()
            .rect;
        let (_, label_y) = output.edges[0].label_pos.unwrap();

        assert!(
            label_y
                > from_rect
                    .y
                    .saturating_add(from_rect.h as i16)
                    .saturating_sub(1)
        );
        assert!(label_y < to_rect.y);
    }

    #[test]
    fn branching_edges_attach_to_top_of_child_layer() {
        // A diamond with two wide siblings on the layer below: the horizontal
        // gap between siblings is larger than the vertical gap, so geometric
        // port selection used to pick East/West sides and render arrows as
        // `◀`/`▶` between the boxes. Layered ports must keep the route on the
        // South→North axis.
        let boxes = vec![
            diagram_box("B", &["Decision"], Vec::new()),
            diagram_box("C", &["Do Something"], Vec::new()),
            diagram_box("D", &["Do Nothing"], Vec::new()),
        ];
        let edges = vec![
            SimpleDiagramEdge {
                from: "B".into(),
                to: "C".into(),
                label: Some("Yes".into()),
                from_label: None,
                to_label: None,
                line_style: Style::default(),
                label_style: Style::default(),
                dashed: false,
                from_glyph: EndpointGlyph::None,
                to_glyph: EndpointGlyph::Arrow,
                prefer_vertical_backedge_labels: true,
            },
            SimpleDiagramEdge {
                from: "B".into(),
                to: "D".into(),
                label: Some("No".into()),
                from_label: None,
                to_label: None,
                line_style: Style::default(),
                label_style: Style::default(),
                dashed: false,
                from_glyph: EndpointGlyph::None,
                to_glyph: EndpointGlyph::Arrow,
                prefer_vertical_backedge_labels: true,
            },
        ];

        let output = build_simple_diagram_output(&boxes, &edges, Padding::default(), 1, 1);
        for edge in &output.edges {
            let target_id = edges[edge.spec_index].to.as_ref();
            let target_rect = output
                .boxes
                .iter()
                .find(|node| boxes[node.spec_index].id.as_ref() == target_id)
                .unwrap()
                .rect;
            let (_, to_y) = edge.to_pos.unwrap();
            assert_eq!(
                to_y, target_rect.y,
                "edge to {target_id} should land on its top face, not its side",
            );
        }
    }

    #[test]
    fn back_edge_routes_outside_forward_lane() {
        let boxes = vec![
            diagram_box("A", &["A"], Vec::new()),
            diagram_box("B", &["B"], Vec::new()),
        ];
        let edges = vec![
            SimpleDiagramEdge {
                from: "A".into(),
                to: "B".into(),
                label: Some("down".into()),
                from_label: None,
                to_label: None,
                line_style: Style::default(),
                label_style: Style::default(),
                dashed: false,
                from_glyph: EndpointGlyph::None,
                to_glyph: EndpointGlyph::Arrow,
                prefer_vertical_backedge_labels: true,
            },
            SimpleDiagramEdge {
                from: "B".into(),
                to: "A".into(),
                label: Some("up".into()),
                from_label: None,
                to_label: None,
                line_style: Style::default(),
                label_style: Style::default(),
                dashed: false,
                from_glyph: EndpointGlyph::None,
                to_glyph: EndpointGlyph::Arrow,
                prefer_vertical_backedge_labels: true,
            },
        ];

        let output = build_simple_diagram_output(&boxes, &edges, Padding::default(), 1, 1);
        let node_right = output
            .boxes
            .iter()
            .map(|node| {
                node.rect
                    .x
                    .saturating_add(node.rect.w as i16)
                    .saturating_sub(1)
            })
            .max()
            .unwrap();
        let forward = output
            .edges
            .iter()
            .find(|edge| edge.spec_index == 0)
            .unwrap();
        let back = output
            .edges
            .iter()
            .find(|edge| edge.spec_index == 1)
            .unwrap();

        assert!(back.cells.iter().any(|cell| cell.x > node_right));
        assert_ne!(forward.label_pos, back.label_pos);
        let label = edges[1].label.as_ref().unwrap();
        let (label_x, label_y) = back.label_pos.unwrap();
        let label_width = UnicodeWidthStr::width(label.as_ref()) as i16;
        for dx in 0..label_width {
            for node in &output.boxes {
                assert!(!node.rect.contains(label_x.saturating_add(dx), label_y));
            }
        }
    }

    fn mermaid_regression_sample() -> (Vec<SimpleDiagramBox>, Vec<SimpleDiagramEdge>, Padding) {
        let boxes = vec![
            diagram_box("A", &["Client Request"], Vec::new()),
            diagram_box("B", &["API Gateway"], Vec::new()),
            diagram_box("C", &["Auth Service"], Vec::new()),
            diagram_box("D", &["Load Balancer"], Vec::new()),
            diagram_box("E", &["401 Unauthorized"], Vec::new()),
            diagram_box("F", &["Service A"], Vec::new()),
            diagram_box("G", &["Service B"], Vec::new()),
            diagram_box("H", &["Service C"], Vec::new()),
            SimpleDiagramBox {
                id: "I".into(),
                rows: vec!["Database".into()],
                divider_after: Vec::new(),
                fill_style: Style::default(),
                border_style_fg: Style::default(),
                label_style: Style::default(),
                border_style: BorderStyle::Plain,
                shape: SimpleDiagramBoxShape::Cylinder,
            },
            diagram_box("J", &["Cache Layer"], Vec::new()),
            diagram_box("K", &["Message Queue"], Vec::new()),
            diagram_box("L", &["Worker Pool"], Vec::new()),
            diagram_box("M", &["Async Processor"], Vec::new()),
        ];
        let edges = vec![
            ("A", "B", None),
            ("B", "C", Some("Authenticate")),
            ("B", "D", Some("Route")),
            ("C", "D", Some("Valid Token")),
            ("C", "E", Some("Invalid")),
            ("D", "F", None),
            ("D", "G", None),
            ("D", "H", None),
            ("F", "I", None),
            ("G", "J", None),
            ("H", "K", None),
            ("K", "L", None),
            ("L", "M", None),
            ("M", "I", None),
            ("J", "F", None),
        ]
        .into_iter()
        .map(|(from, to, label)| SimpleDiagramEdge {
            from: from.into(),
            to: to.into(),
            label: label.map(Arc::from),
            from_label: None,
            to_label: None,
            line_style: Style::default(),
            label_style: Style::default(),
            dashed: false,
            from_glyph: EndpointGlyph::None,
            to_glyph: EndpointGlyph::Arrow,
            prefer_vertical_backedge_labels: true,
        })
        .collect();
        (
            boxes,
            edges,
            Padding {
                top: 0,
                right: 1,
                bottom: 0,
                left: 1,
            },
        )
    }

    #[test]
    fn routes_and_labels_avoid_boxes_in_mermaid_regression_graph() {
        let (boxes, edges, padding) = mermaid_regression_sample();
        let output = build_simple_diagram_output(&boxes, &edges, padding, 4, 4);
        let id_to_rect = output
            .boxes
            .iter()
            .filter_map(|node| {
                boxes
                    .get(node.spec_index)
                    .map(|spec| (spec.id.as_ref(), node.rect))
            })
            .collect::<HashMap<_, _>>();

        for edge in &output.edges {
            let spec = &edges[edge.spec_index];
            for cell in &edge.cells {
                for (id, rect) in &id_to_rect {
                    if rect.contains(cell.x, cell.y) {
                        let is_endpoint_box = *id == spec.from.as_ref() || *id == spec.to.as_ref();
                        assert!(
                            is_endpoint_box && is_rect_border_cell(*rect, cell.x, cell.y),
                            "edge {} -> {} crossed box {id} at ({}, {})",
                            spec.from,
                            spec.to,
                            cell.x,
                            cell.y,
                        );
                    }
                }
            }
        }

        let mut label_spans = Vec::new();
        for edge in &output.edges {
            let Some(label) = edges
                .get(edge.spec_index)
                .and_then(|spec| spec.label.as_ref())
            else {
                continue;
            };
            let Some((x, y)) = edge.label_pos else {
                continue;
            };
            let width = UnicodeWidthStr::width(label.as_ref()) as i16;
            for dx in 0..width {
                for rect in id_to_rect.values() {
                    assert!(
                        !rect.contains(x.saturating_add(dx), y),
                        "label {label:?} overlapped box at ({}, {})",
                        x.saturating_add(dx),
                        y,
                    );
                }
            }
            label_spans.push((label.clone(), x, y, width));
        }

        for i in 0..label_spans.len() {
            for j in i + 1..label_spans.len() {
                let (left_label, left_x, left_y, left_w) = &label_spans[i];
                let (right_label, right_x, right_y, right_w) = &label_spans[j];
                if left_y == right_y {
                    let left_end = left_x.saturating_add(*left_w);
                    let right_end = right_x.saturating_add(*right_w);
                    assert!(
                        left_end <= *right_x || right_end <= *left_x,
                        "labels {left_label:?} and {right_label:?} overlapped on row {left_y}",
                    );
                }
            }
        }

        let service_b_lane_y =
            horizontal_lane_y(&output, &edges, "D", "G").expect("D -> G horizontal lane");
        let service_c_lane_y =
            horizontal_lane_y(&output, &edges, "D", "H").expect("D -> H horizontal lane");
        assert!(
            service_c_lane_y < service_b_lane_y,
            "D -> H should use the free row above D -> G instead of overlapping or curling",
        );
        let cache_to_service_a_lane_y = horizontal_lane_y(&output, &edges, "J", "F")
            .expect("J -> F horizontal target approach lane");
        assert_ne!(
            cache_to_service_a_lane_y, service_b_lane_y,
            "Cache Layer -> Service A should not share the Service B branch lane",
        );
        assert_ne!(
            cache_to_service_a_lane_y, service_c_lane_y,
            "Cache Layer -> Service A should not share the Service C branch lane",
        );
        let service_a_targets = output
            .edges
            .iter()
            .filter(|edge| edges[edge.spec_index].to.as_ref() == "F")
            .map(|edge| edge.to_pos.expect("Service A target port"))
            .collect::<Vec<_>>();
        assert_eq!(service_a_targets.len(), 2);
        assert_ne!(
            service_a_targets[0], service_a_targets[1],
            "Load Balancer and Cache Layer should not share one Service A arrowhead",
        );
        assert_arrow_landing_cells_have_clear_stems(&output, &boxes, &edges);
    }

    fn assert_arrow_landing_cells_have_clear_stems(
        output: &SimpleDiagramOutput,
        boxes: &[SimpleDiagramBox],
        edges: &[SimpleDiagramEdge],
    ) {
        let id_to_rect = output
            .boxes
            .iter()
            .filter_map(|node| {
                boxes
                    .get(node.spec_index)
                    .map(|spec| (spec.id.as_ref(), node.rect))
            })
            .collect::<HashMap<_, _>>();
        let mut merged = HashMap::new();
        for edge in &output.edges {
            for cell in &edge.cells {
                let entry = merged.entry((cell.x, cell.y)).or_insert(0u8);
                *entry |= cell.bits;
            }
        }

        for edge in &output.edges {
            let spec = &edges[edge.spec_index];
            if matches!(spec.from_glyph, EndpointGlyph::Arrow) {
                let rect = id_to_rect
                    .get(spec.from.as_ref())
                    .copied()
                    .expect("source rect");
                let (x, y) = edge.from_pos.expect("source endpoint");
                assert_arrow_landing_cell_has_clear_stem(
                    &merged,
                    rect,
                    x,
                    y,
                    &format!("{} -> {} source", spec.from, spec.to),
                );
            }
            if matches!(spec.to_glyph, EndpointGlyph::Arrow) {
                let rect = id_to_rect
                    .get(spec.to.as_ref())
                    .copied()
                    .expect("target rect");
                let (x, y) = edge.to_pos.expect("target endpoint");
                assert_arrow_landing_cell_has_clear_stem(
                    &merged,
                    rect,
                    x,
                    y,
                    &format!("{} -> {} target", spec.from, spec.to),
                );
            }
        }
    }

    fn assert_arrow_landing_cell_has_clear_stem(
        merged: &HashMap<(i16, i16), u8>,
        rect: Rect,
        x: i16,
        y: i16,
        edge_name: &str,
    ) {
        let (outside_x, outside_y, side) = outside_endpoint_side(x, y, rect);
        let bits = merged.get(&(outside_x, outside_y)).copied().unwrap_or(0);
        let perpendicular = match side {
            EndpointSide::North | EndpointSide::South => EAST | WEST,
            EndpointSide::East | EndpointSide::West => NORTH | SOUTH,
        };
        assert_eq!(
            bits & perpendicular,
            0,
            "arrow landing cell for {edge_name} has a perpendicular route at ({outside_x}, {outside_y})",
        );
    }

    fn horizontal_lane_y(
        output: &SimpleDiagramOutput,
        edges: &[SimpleDiagramEdge],
        from: &str,
        to: &str,
    ) -> Option<i16> {
        output
            .edges
            .iter()
            .find(|edge| {
                let spec = &edges[edge.spec_index];
                spec.from.as_ref() == from && spec.to.as_ref() == to
            })?
            .cells
            .iter()
            .filter_map(|cell| ((cell.bits & (EAST | WEST)) == (EAST | WEST)).then_some(cell.y))
            .min()
    }
}
