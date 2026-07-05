use std::collections::HashMap;

use crate::style::Rect;

use super::{
    DagInput, DagNodeId, DagPoint, DagPort, PositionedNode, RoutedEdge, WorkingEdge,
    ports::{self, PortSide},
};

const BACK_EDGE_LANE_CLEARANCE: i16 = 4;
const BACK_EDGE_LANE_SPACING: i16 = 3;
const BACK_EDGE_EXIT_CLEARANCE: i16 = 2;
const PORT_SPACING: i16 = 2;

pub(super) fn route_edges(
    input: &DagInput,
    nodes: &[PositionedNode],
    edges: &[WorkingEdge],
) -> Vec<RoutedEdge> {
    let by_id: HashMap<&DagNodeId, &PositionedNode> =
        nodes.iter().map(|node| (&node.id, node)).collect();
    let all_obstacles = nodes.iter().map(|node| node.rect).collect::<Vec<_>>();
    let max_right = nodes
        .iter()
        .map(|node| right_edge(node.rect))
        .max()
        .unwrap_or(0);
    // Source-aware occupancy: per-cell, count visits from each source separately.
    // Other sources are hard avoidance; the same source is a soft tie-breaker so
    // sibling fan-out can share simple stems but will use a nearby free lane when
    // it costs the same number of bends.
    let mut occupancy: HashMap<(i16, i16), HashMap<DagNodeId, u16>> = HashMap::new();
    let mut back_edge_slot = 0i16;
    let mut target_back_edge_slot: HashMap<DagNodeId, i16> = HashMap::new();
    let mut source_back_edge_slot: HashMap<i16, i16> = HashMap::new();
    let port_assignments = compute_port_assignments(input, &by_id, edges);
    let mut routed = vec![None; edges.len()];
    let mut route_order: Vec<_> = edges.iter().enumerate().collect();
    route_order.sort_by_key(|(edge_index, edge)| {
        let spec = &input.edges[edge.spec_index];
        let from = by_id.get(&spec.from).copied();
        let to = by_id.get(&spec.to).copied();
        (
            edge.reversed,
            from.map(|node| node.layer).unwrap_or(usize::MAX),
            from.map(|node| node.order).unwrap_or(usize::MAX),
            to.map(|node| node.layer).unwrap_or(usize::MAX),
            to.map(|node| node.order).unwrap_or(usize::MAX),
            *edge_index,
        )
    });

    for (edge_index, edge) in route_order {
        let spec = &input.edges[edge.spec_index];
        let (Some(from), Some(to)) = (by_id.get(&spec.from), by_id.get(&spec.to)) else {
            continue;
        };
        let obstacles = all_obstacles
            .iter()
            .copied()
            .filter(|rect| *rect != from.rect && *rect != to.rect)
            .collect::<Vec<_>>();
        let (from_port, to_port, points) = if edge.reversed {
            let lane_x = max_right
                .saturating_add(BACK_EDGE_LANE_CLEARANCE)
                .saturating_add(back_edge_slot.saturating_mul(BACK_EDGE_LANE_SPACING));
            back_edge_slot = back_edge_slot.saturating_add(1);
            let slots = BackEdgeSlots {
                approach: {
                    let entry = target_back_edge_slot.entry(spec.to.clone()).or_insert(0);
                    let s = *entry;
                    *entry = entry.saturating_add(1);
                    s
                },
                exit: {
                    let base_exit_row = if center_y(from.rect) >= center_y(to.rect) {
                        bottom_edge(from.rect).saturating_add(1)
                    } else {
                        from.rect.y.saturating_sub(1)
                    };
                    let entry = source_back_edge_slot.entry(base_exit_row).or_insert(0);
                    let s = *entry;
                    *entry = entry.saturating_add(1);
                    s
                },
            };
            back_edge_route(
                from.rect, to.rect, lane_x, slots, &obstacles, &occupancy, &spec.from,
            )
        } else {
            let (from_port, to_port) = port_assignments
                .get(&edge_index)
                .copied()
                .unwrap_or_else(|| layered_connect(from, to));
            let axis = if from.layer < to.layer {
                RouteAxis::VerticalDown
            } else if from.layer > to.layer {
                RouteAxis::VerticalUp
            } else if from.rect.x <= to.rect.x {
                RouteAxis::HorizontalRight
            } else {
                RouteAxis::HorizontalLeft
            };
            let points = route_forward_points(
                from_port.point,
                to_port.point,
                axis,
                &obstacles,
                &occupancy,
                &spec.from,
            );
            (from_port, to_port, points)
        };
        mark_route_occupancy(&points, &spec.from, &mut occupancy);
        routed[edge_index] = Some(RoutedEdge {
            from: spec.from.clone(),
            to: spec.to.clone(),
            kind: spec.kind,
            label: spec.label.clone(),
            points,
            from_port,
            to_port,
            reversed: edge.reversed,
            head_from: spec.head_from,
            head_to: spec.head_to,
        });
    }

    routed.into_iter().flatten().collect()
}

// The DAG places layers top-to-bottom (see `coords::assign_coordinates`), so any
// edge between two different layers should attach to the South face of the
// upper-layer node and the North face of the lower-layer one. Falling back to
// `ports::connect`'s geometric heuristic flips ports to East/West whenever
// siblings sit far enough apart horizontally — making branches appear to enter
// targets from the side and rendering endpoint glyphs as `◀`/`▶` instead of `▼`.
fn layered_connect(from: &PositionedNode, to: &PositionedNode) -> (DagPort, DagPort) {
    if from.layer < to.layer {
        (
            ports::port(from.rect, PortSide::South),
            ports::port(to.rect, PortSide::North),
        )
    } else if from.layer > to.layer {
        (
            ports::port(from.rect, PortSide::North),
            ports::port(to.rect, PortSide::South),
        )
    } else {
        ports::connect(from.rect, to.rect)
    }
}

/// Pre-pass: when multiple forward edges leave or enter the same node-face,
/// give each edge its own port column/row aligned with the OPPOSITE end's
/// position so the route can drop straight down (or run straight across)
/// with no horizontal jog. Falls back to even distribution when alignment
/// would collide. Same-layer edges keep `ports::connect`'s heuristic.
fn compute_port_assignments(
    input: &DagInput,
    by_id: &HashMap<&DagNodeId, &PositionedNode>,
    edges: &[WorkingEdge],
) -> HashMap<usize, (DagPort, DagPort)> {
    let mut from_groups: HashMap<(DagNodeId, PortSide), Vec<usize>> = HashMap::new();
    let mut to_groups: HashMap<(DagNodeId, PortSide), Vec<usize>> = HashMap::new();
    for (edge_index, edge) in edges.iter().enumerate() {
        if edge.reversed {
            continue;
        }
        let spec = &input.edges[edge.spec_index];
        let (Some(_), Some(_)) = (by_id.get(&spec.from), by_id.get(&spec.to)) else {
            continue;
        };
        let from_node = by_id[&spec.from];
        let to_node = by_id[&spec.to];
        let (from_side, to_side) = match from_node.layer.cmp(&to_node.layer) {
            std::cmp::Ordering::Less => (PortSide::South, PortSide::North),
            std::cmp::Ordering::Greater => (PortSide::North, PortSide::South),
            std::cmp::Ordering::Equal => continue,
        };
        from_groups
            .entry((spec.from.clone(), from_side))
            .or_default()
            .push(edge_index);
        to_groups
            .entry((spec.to.clone(), to_side))
            .or_default()
            .push(edge_index);
    }

    let mut from_ports: HashMap<usize, DagPort> = HashMap::new();
    for ((source_id, side), edge_indices) in &from_groups {
        let source = by_id[source_id];
        let resolved = resolve_face_ports(source.rect, *side, edge_indices, false, |edge_idx| {
            let to_id = &input.edges[edges[edge_idx].spec_index].to;
            opposite_axis_position(by_id.get(to_id).copied(), *side)
        });
        for (edge_idx, port) in resolved {
            from_ports.insert(edge_idx, port);
        }
    }

    // Phase 2: target ports align to each edge's already-computed SOURCE PORT
    // position (not the source node's center). When source.center and
    // target.center disagree by even one column, this lets a single-edge route
    // drop straight down without a 1-cell jog.
    let mut to_ports: HashMap<usize, DagPort> = HashMap::new();
    for ((target_id, side), edge_indices) in &to_groups {
        let target = by_id[target_id];
        // Incoming ports whose desired columns are closer than PORT_SPACING are
        // going to be spread artificially. In that tie case, keep declaration
        // order instead of treating a one-cell source-port difference as
        // meaningful; skip edges that detour around an intervening node then
        // keep the outside target port instead of crossing the node's own edge.
        let resolved = resolve_face_ports(target.rect, *side, edge_indices, true, |edge_idx| {
            let port = from_ports.get(&edge_idx)?;
            Some(match side {
                PortSide::North | PortSide::South => port.point.x,
                PortSide::East | PortSide::West => port.point.y,
            })
        });
        for (edge_idx, port) in resolved {
            to_ports.insert(edge_idx, port);
        }
    }

    let mut assignments = HashMap::new();
    for edge_index in 0..edges.len() {
        if let (Some(&from_port), Some(&to_port)) =
            (from_ports.get(&edge_index), to_ports.get(&edge_index))
        {
            assignments.insert(edge_index, (from_port, to_port));
        }
    }
    assignments
}

/// Position of the opposite end's center along the axis of `side`'s face
/// (column for North/South, row for East/West). Returns `None` when the
/// opposite node isn't positioned.
fn opposite_axis_position(opposite: Option<&PositionedNode>, side: PortSide) -> Option<i16> {
    let node = opposite?;
    Some(match side {
        PortSide::North | PortSide::South => node.rect.x.saturating_add((node.rect.w / 2) as i16),
        PortSide::East | PortSide::West => node.rect.y.saturating_add((node.rect.h / 2) as i16),
    })
}

/// For a single (node, face) group of edges: try column-alignment with each
/// edge's opposite end, then spread any colliding ports apart while keeping
/// the non-colliding ones in their aligned positions. Falls back to even
/// distribution only when the group can't fit in the face's interior.
fn resolve_face_ports<F>(
    rect: Rect,
    side: PortSide,
    edge_indices: &[usize],
    input_order_for_close_ports: bool,
    desired_position: F,
) -> Vec<(usize, DagPort)>
where
    F: Fn(usize) -> Option<i16>,
{
    if edge_indices.len() == 1 {
        let edge_idx = edge_indices[0];
        let port = match desired_position(edge_idx) {
            Some(pos) => ports::port_aligned(rect, side, pos),
            None => ports::port(rect, side),
        };
        return vec![(edge_idx, port)];
    }

    let (interior_lo, interior_hi) = match side {
        PortSide::North | PortSide::South => (
            rect.x.saturating_add(1),
            rect.x.saturating_add(rect.w.saturating_sub(2) as i16),
        ),
        PortSide::East | PortSide::West => (
            rect.y.saturating_add(1),
            rect.y.saturating_add(rect.h.saturating_sub(2) as i16),
        ),
    };

    let mut indexed: Vec<(usize, i16, usize, i16)> = edge_indices
        .iter()
        .enumerate()
        .map(|(input_order, &idx)| {
            let raw_pos = desired_position(idx).unwrap_or((interior_lo + interior_hi) / 2);
            let pos = raw_pos.clamp(interior_lo, interior_hi);
            (idx, pos, input_order, raw_pos)
        })
        .collect();

    if input_order_for_close_ports {
        let min_pos = indexed.iter().map(|(_, pos, _, _)| *pos).min().unwrap_or(0);
        let max_pos = indexed.iter().map(|(_, pos, _, _)| *pos).max().unwrap_or(0);
        let spread = PORT_SPACING.saturating_mul(indexed.len().saturating_sub(1) as i16);
        if min_pos.abs_diff(max_pos) < spread as u16 {
            indexed.sort_by_key(|(idx, _, input_order, _)| (*input_order, *idx));
            let last_index = indexed.len().saturating_sub(1);
            for (i, (_, pos, _, _)) in indexed.iter_mut().enumerate() {
                let from_right = last_index.saturating_sub(i) as i16;
                *pos = min_pos.saturating_sub(PORT_SPACING.saturating_mul(from_right));
            }

            let current_min = indexed.iter().map(|(_, pos, _, _)| *pos).min().unwrap_or(0);
            let current_max = indexed.iter().map(|(_, pos, _, _)| *pos).max().unwrap_or(0);
            let mut shift = if current_min < interior_lo {
                interior_lo.saturating_sub(current_min)
            } else {
                0
            };
            if current_max.saturating_add(shift) > interior_hi {
                shift = shift.saturating_sub(
                    current_max
                        .saturating_add(shift)
                        .saturating_sub(interior_hi),
                );
            }
            if shift != 0 {
                for (_, pos, _, _) in &mut indexed {
                    *pos = pos.saturating_add(shift);
                }
            }

            if indexed
                .iter()
                .all(|(_, pos, _, _)| *pos >= interior_lo && *pos <= interior_hi)
            {
                return indexed
                    .into_iter()
                    .map(|(idx, pos, _, _)| (idx, ports::port_aligned(rect, side, pos)))
                    .collect();
            }
        }
    }

    indexed.sort_by(|a, b| {
        let close = a.1.abs_diff(b.1) < PORT_SPACING as u16;
        if input_order_for_close_ports && close {
            a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0))
        } else {
            a.3.cmp(&b.3).then_with(|| a.0.cmp(&b.0))
        }
    });

    // 2-cell minimum spacing: leaves a clear empty column between adjacent
    // sibling stems so a far-east traversal doesn't collide with an
    // intermediate stem at the gutter junction (the `└┼` artifact).

    // Forward pass: ensure adjacent positions are at least PORT_SPACING apart.
    for i in 1..indexed.len() {
        let min_pos = indexed[i - 1].1.saturating_add(PORT_SPACING);
        if indexed[i].1 < min_pos {
            indexed[i].1 = min_pos;
        }
    }
    // If we overflowed the right edge, walk backward and pull each position
    // leftward; if that pushes the leftmost below the interior, the group
    // simply doesn't fit and we fall back to even distribution.
    if indexed
        .last()
        .map(|(_, pos, _, _)| *pos > interior_hi)
        .unwrap_or(false)
    {
        let last_idx = indexed.len() - 1;
        indexed[last_idx].1 = interior_hi;
        for i in (0..last_idx).rev() {
            let max_pos = indexed[i + 1].1.saturating_sub(PORT_SPACING);
            if indexed[i].1 > max_pos {
                indexed[i].1 = max_pos;
            }
        }
        if indexed[0].1 < interior_lo {
            let count = indexed.len() as u16;
            return indexed
                .into_iter()
                .enumerate()
                .map(|(i, (idx, _, _, _))| {
                    (idx, ports::port_distributed(rect, side, i as u16, count))
                })
                .collect();
        }
    }

    indexed
        .into_iter()
        .map(|(idx, pos, _, _)| (idx, ports::port_aligned(rect, side, pos)))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RouteAxis {
    VerticalDown,
    VerticalUp,
    HorizontalRight,
    HorizontalLeft,
}

impl RouteAxis {
    fn is_vertical(self) -> bool {
        matches!(self, Self::VerticalDown | Self::VerticalUp)
    }
}

#[derive(Clone, Debug)]
struct RouteCandidate {
    points: Vec<DagPoint>,
    occupancy_cost: u32,
    same_source_overlap_cost: u32,
    bend_count: usize,
    deviation: i32,
}

fn route_forward_points(
    start: DagPoint,
    end: DagPoint,
    axis: RouteAxis,
    obstacles: &[Rect],
    occupancy: &HashMap<(i16, i16), HashMap<DagNodeId, u16>>,
    source: &DagNodeId,
) -> Vec<DagPoint> {
    let mut candidates = Vec::new();

    if segment_clear(start, end, obstacles) {
        candidates.push(RouteCandidate::new(
            compact_points(vec![start, end]),
            occupancy,
            source,
            0,
        ));
    }

    if axis.is_vertical() {
        let midpoint = signed_midpoint(start.y, end.y);
        // Reserve the row immediately adjacent to the target port as a clear
        // arrow stem so endpoint glyphs don't overwrite junction corners at
        // fan-in/fan-out. Falls back to the unfiltered set when no other
        // candidate is available (very tight layouts).
        let stem_row = match axis {
            RouteAxis::VerticalDown => end.y.saturating_sub(1),
            RouteAxis::VerticalUp => end.y.saturating_add(1),
            _ => unreachable!(),
        };
        let raw = axis_values_between(start.y, end.y);
        let preferred: Vec<i16> = raw.iter().copied().filter(|y| *y != stem_row).collect();
        let gutters = if preferred.is_empty() { raw } else { preferred };
        let local_branch_reference_y = local_branch_reference_y(start, end, axis);
        for gutter_y in gutters {
            let points = compact_points(vec![
                start,
                DagPoint::new(start.x, gutter_y),
                DagPoint::new(end.x, gutter_y),
                end,
            ]);
            if path_clear(&points, obstacles) {
                candidates.push(RouteCandidate::new(
                    points,
                    occupancy,
                    source,
                    i32::from(
                        gutter_y
                            .saturating_sub(local_branch_reference_y.unwrap_or(midpoint))
                            .abs(),
                    ),
                ));
            }
        }

        let exit_y = match axis {
            RouteAxis::VerticalDown => start.y.saturating_add(1),
            RouteAxis::VerticalUp => start.y.saturating_sub(1),
            _ => unreachable!(),
        };
        let midpoint = signed_midpoint(start.x, end.x);
        for approach_y in target_approach_values(start.y, end.y, axis) {
            for detour_x in detour_minor_candidates(start, end, obstacles, true) {
                let points = compact_points(vec![
                    start,
                    DagPoint::new(start.x, exit_y),
                    DagPoint::new(detour_x, exit_y),
                    DagPoint::new(detour_x, approach_y),
                    DagPoint::new(end.x, approach_y),
                    end,
                ]);
                if path_clear(&points, obstacles) {
                    candidates.push(RouteCandidate::new(
                        points,
                        occupancy,
                        source,
                        i32::from(detour_x.saturating_sub(midpoint).abs()),
                    ));
                }
            }
        }
    } else {
        let midpoint = signed_midpoint(start.x, end.x);
        let stem_col = match axis {
            RouteAxis::HorizontalRight => end.x.saturating_sub(1),
            RouteAxis::HorizontalLeft => end.x.saturating_add(1),
            _ => unreachable!(),
        };
        let raw = axis_values_between(start.x, end.x);
        let preferred: Vec<i16> = raw.iter().copied().filter(|x| *x != stem_col).collect();
        let gutters = if preferred.is_empty() { raw } else { preferred };
        for gutter_x in gutters {
            let points = compact_points(vec![
                start,
                DagPoint::new(gutter_x, start.y),
                DagPoint::new(gutter_x, end.y),
                end,
            ]);
            if path_clear(&points, obstacles) {
                candidates.push(RouteCandidate::new(
                    points,
                    occupancy,
                    source,
                    i32::from(gutter_x.saturating_sub(midpoint).abs()),
                ));
            }
        }

        let exit_x = match axis {
            RouteAxis::HorizontalRight => start.x.saturating_add(1),
            RouteAxis::HorizontalLeft => start.x.saturating_sub(1),
            _ => unreachable!(),
        };
        let midpoint = signed_midpoint(start.y, end.y);
        for approach_x in target_approach_values(start.x, end.x, axis) {
            for detour_y in detour_minor_candidates(start, end, obstacles, false) {
                let points = compact_points(vec![
                    start,
                    DagPoint::new(exit_x, start.y),
                    DagPoint::new(exit_x, detour_y),
                    DagPoint::new(approach_x, detour_y),
                    DagPoint::new(approach_x, end.y),
                    end,
                ]);
                if path_clear(&points, obstacles) {
                    candidates.push(RouteCandidate::new(
                        points,
                        occupancy,
                        source,
                        i32::from(detour_y.saturating_sub(midpoint).abs()),
                    ));
                }
            }
        }
    }

    candidates
        .into_iter()
        .min_by_key(|candidate| {
            (
                candidate.occupancy_cost,
                candidate.bend_count,
                candidate.same_source_overlap_cost,
                candidate.deviation,
            )
        })
        .map(|candidate| candidate.points)
        .unwrap_or_else(|| fallback_orthogonal_points(start, end, axis))
}

impl RouteCandidate {
    fn new(
        points: Vec<DagPoint>,
        occupancy: &HashMap<(i16, i16), HashMap<DagNodeId, u16>>,
        source: &DagNodeId,
        deviation: i32,
    ) -> Self {
        let (occupancy_cost, same_source_overlap_cost) =
            path_overlap_costs(&points, occupancy, source);
        Self {
            occupancy_cost,
            same_source_overlap_cost,
            bend_count: points.len().saturating_sub(2),
            points,
            deviation,
        }
    }
}

fn fallback_orthogonal_points(from: DagPoint, to: DagPoint, axis: RouteAxis) -> Vec<DagPoint> {
    if from.x == to.x || from.y == to.y {
        return vec![from, to];
    }

    match axis {
        RouteAxis::VerticalDown | RouteAxis::VerticalUp => {
            let mid_y = signed_midpoint(from.y, to.y);
            vec![
                from,
                DagPoint::new(from.x, mid_y),
                DagPoint::new(to.x, mid_y),
                to,
            ]
        }
        RouteAxis::HorizontalRight | RouteAxis::HorizontalLeft => {
            let mid_x = signed_midpoint(from.x, to.x);
            vec![
                from,
                DagPoint::new(mid_x, from.y),
                DagPoint::new(mid_x, to.y),
                to,
            ]
        }
    }
}

fn target_approach_values(start_axis: i16, end_axis: i16, axis: RouteAxis) -> Vec<i16> {
    // Prefer the lane one cell before the endpoint stem so the arrowhead cell
    // carries only straight-line bits. Use the adjacent stem row/column only in
    // very tight layouts where no clear lane exists.
    let (clear_stem, fallback_stem) = match axis {
        RouteAxis::VerticalDown | RouteAxis::HorizontalRight => {
            (end_axis.saturating_sub(2), end_axis.saturating_sub(1))
        }
        RouteAxis::VerticalUp | RouteAxis::HorizontalLeft => {
            (end_axis.saturating_add(2), end_axis.saturating_add(1))
        }
    };
    if strictly_between(clear_stem, start_axis, end_axis) {
        vec![clear_stem]
    } else if strictly_between(fallback_stem, start_axis, end_axis) {
        vec![fallback_stem]
    } else {
        Vec::new()
    }
}

fn strictly_between(value: i16, a: i16, b: i16) -> bool {
    let (lo, hi) = ordered_pair(a, b);
    value > lo && value < hi
}

fn back_edge_route(
    from: Rect,
    to: Rect,
    lane_x: i16,
    slots: BackEdgeSlots,
    obstacles: &[Rect],
    occupancy: &HashMap<(i16, i16), HashMap<DagNodeId, u16>>,
    source: &DagNodeId,
) -> (DagPort, DagPort, Vec<DagPoint>) {
    // Prefer the source's South/North face — that gives a clean run along the
    // box edge into the lane with no parallel-channel artifact. Fall back to
    // the East-face exit when the South-face path would cut through another
    // node (e.g., a back-edge whose source sits above other forward-laid
    // nodes that share the lane row).
    //
    // `approach_slot` is the 0-based index of this back-edge among all
    // back-edges that point at the same target. It biases the candidate list
    // toward a row that prior siblings have not already occupied — without
    // it, every back-edge to the same target would compete for the same 4
    // rows and collapse onto a shared trunk.
    //
    // `exit_slot` is the 0-based index of this back-edge among all back-edges
    // that leave through the same source exit row. It keeps distinct feedback
    // edges from different sources on the same layer from collapsing into a
    // single horizontal run before they peel off toward separate targets.
    let mut best_path = None;
    let exit_offset = slots.exit.max(0);
    for target_distance in back_edge_distance_candidates(slots.approach) {
        let south_path = south_face_back_edge(from, to, lane_x, target_distance, exit_offset);
        if path_clear(&south_path.2, obstacles) {
            if let Some(path) = update_back_edge_best(&mut best_path, south_path, occupancy, source)
            {
                return path;
            }
        }
        let east_path = east_face_back_edge(from, to, lane_x, target_distance, exit_offset);
        if path_clear(&east_path.2, obstacles) {
            if let Some(path) = update_back_edge_best(&mut best_path, east_path, occupancy, source)
            {
                return path;
            }
        }
    }
    best_path
        .map(|(_, path)| path)
        .unwrap_or_else(|| east_face_back_edge(from, to, lane_x, 1, exit_offset))
}

#[derive(Clone, Copy, Debug)]
struct BackEdgeSlots {
    approach: i16,
    exit: i16,
}

fn back_edge_distance_candidates(approach_slot: i16) -> Vec<i16> {
    // Slot 0 preserves the historical `[4, 3, 2, 1]` candidate order (far-from-
    // target first, falling closer if blocked). Each additional back-edge
    // pointing at the same target extends the upper bound by one row so the
    // occupancy-based selector can pick an un-shared lane.
    let max_distance = 4i16.saturating_add(approach_slot.max(0));
    (1..=max_distance).rev().collect()
}

type BackEdgePath = (DagPort, DagPort, Vec<DagPoint>);
type ScoredBackEdgePath = (u32, BackEdgePath);

fn update_back_edge_best(
    current: &mut Option<ScoredBackEdgePath>,
    candidate: BackEdgePath,
    occupancy: &HashMap<(i16, i16), HashMap<DagNodeId, u16>>,
    source: &DagNodeId,
) -> Option<BackEdgePath> {
    let candidate = best_scored_path(candidate, occupancy, source);
    if candidate.0 == 0 {
        return Some(candidate.1);
    }
    if current
        .as_ref()
        .map(|current| candidate.0 < current.0)
        .unwrap_or(true)
    {
        *current = Some(candidate);
    }
    None
}

fn best_scored_path(
    path: BackEdgePath,
    occupancy: &HashMap<(i16, i16), HashMap<DagNodeId, u16>>,
    source: &DagNodeId,
) -> ScoredBackEdgePath {
    let (other_source, same_source) = path_overlap_costs(&path.2, occupancy, source);
    (
        other_source.saturating_mul(100).saturating_add(same_source),
        path,
    )
}

fn south_face_back_edge(
    from: Rect,
    to: Rect,
    lane_x: i16,
    target_distance: i16,
    exit_offset: i16,
) -> (DagPort, DagPort, Vec<DagPoint>) {
    let source_below_target = center_y(from) >= center_y(to);
    let (from_port, to_port, exit_y, approach_y) = if source_below_target {
        (
            ports::port(from, PortSide::South),
            ports::port_aligned(to, PortSide::North, lane_x),
            bottom_edge(from)
                .saturating_add(1)
                .saturating_add(exit_offset),
            target_back_edge_approach(to.y, true, target_distance),
        )
    } else {
        (
            ports::port(from, PortSide::North),
            ports::port_aligned(to, PortSide::South, lane_x),
            from.y.saturating_sub(1).saturating_sub(exit_offset),
            target_back_edge_approach(bottom_edge(to), false, target_distance),
        )
    };
    let points = compact_points(vec![
        from_port.point,
        DagPoint::new(from_port.point.x, exit_y),
        DagPoint::new(lane_x, exit_y),
        DagPoint::new(lane_x, approach_y),
        DagPoint::new(to_port.point.x, approach_y),
        to_port.point,
    ]);
    (from_port, to_port, points)
}

fn east_face_back_edge(
    from: Rect,
    to: Rect,
    lane_x: i16,
    target_distance: i16,
    exit_offset: i16,
) -> (DagPort, DagPort, Vec<DagPoint>) {
    let from_port = ports::port(from, PortSide::East);
    let source_below_target = center_y(from) >= center_y(to);
    let exit_x = right_edge(from).saturating_add(BACK_EDGE_EXIT_CLEARANCE);
    let (to_port, exit_y, approach_y) = if source_below_target {
        (
            ports::port_aligned(to, PortSide::North, lane_x),
            bottom_edge(from)
                .saturating_add(1)
                .saturating_add(exit_offset),
            target_back_edge_approach(to.y, true, target_distance),
        )
    } else {
        (
            ports::port_aligned(to, PortSide::South, lane_x),
            from.y.saturating_sub(1).saturating_sub(exit_offset),
            target_back_edge_approach(bottom_edge(to), false, target_distance),
        )
    };
    let points = compact_points(vec![
        from_port.point,
        DagPoint::new(exit_x, from_port.point.y),
        DagPoint::new(exit_x, exit_y),
        DagPoint::new(lane_x, exit_y),
        DagPoint::new(lane_x, approach_y),
        DagPoint::new(to_port.point.x, approach_y),
        to_port.point,
    ]);
    (from_port, to_port, points)
}

fn target_back_edge_approach(target_edge: i16, target_above_route: bool, distance: i16) -> i16 {
    if target_above_route {
        target_edge.saturating_sub(distance)
    } else {
        target_edge.saturating_add(distance)
    }
}

fn right_edge(rect: Rect) -> i16 {
    rect.x.saturating_add(rect.w.saturating_sub(1) as i16)
}

fn bottom_edge(rect: Rect) -> i16 {
    rect.y.saturating_add(rect.h.saturating_sub(1) as i16)
}

fn center_y(rect: Rect) -> i16 {
    rect.y.saturating_add((rect.h / 2) as i16)
}

fn local_branch_reference_y(start: DagPoint, end: DagPoint, axis: RouteAxis) -> Option<i16> {
    // For short left/right fan-outs, prefer the free row just before the target
    // stem. That keeps local relations below longer trunks that naturally use
    // the midpoint row, avoiding avoidable crossings when the long edge is
    // routed later.
    if start.x.abs_diff(end.x) > 4 {
        return None;
    }

    match axis {
        RouteAxis::VerticalDown => Some(end.y.saturating_sub(2)),
        RouteAxis::VerticalUp => Some(end.y.saturating_add(2)),
        RouteAxis::HorizontalRight | RouteAxis::HorizontalLeft => None,
    }
}

fn axis_values_between(a: i16, b: i16) -> Vec<i16> {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let midpoint = signed_midpoint(a, b);
    let mut values = (lo.saturating_add(1)..hi).collect::<Vec<_>>();
    values.sort_by_key(|value| {
        (
            i32::from(value.saturating_sub(midpoint).abs()),
            i32::from(value.saturating_sub(lo).abs()),
            *value,
        )
    });
    values
}

fn detour_minor_candidates(
    start: DagPoint,
    end: DagPoint,
    obstacles: &[Rect],
    vertical_axis: bool,
) -> Vec<i16> {
    let (axis_lo, axis_hi) = if vertical_axis {
        ordered_pair(start.y, end.y)
    } else {
        ordered_pair(start.x, end.x)
    };
    let center_minor = if vertical_axis {
        signed_midpoint(start.x, end.x)
    } else {
        signed_midpoint(start.y, end.y)
    };
    let mut values = Vec::new();
    let mut push = |value: i16| {
        if value >= 0 && !values.contains(&value) {
            values.push(value);
        }
    };

    for rect in obstacles.iter().copied().filter(|rect| {
        if vertical_axis {
            rect.y.saturating_add(rect.h as i16) > axis_lo && rect.y < axis_hi
        } else {
            rect.x.saturating_add(rect.w as i16) > axis_lo && rect.x < axis_hi
        }
    }) {
        if vertical_axis {
            push(rect.x.saturating_sub(1));
            push(rect.x.saturating_add(rect.w as i16));
        } else {
            push(rect.y.saturating_sub(1));
            push(rect.y.saturating_add(rect.h as i16));
        }
    }

    let base_minor = if vertical_axis {
        start.x.min(end.x)
    } else {
        start.y.min(end.y)
    };
    for delta in 1..=8 {
        push(base_minor.saturating_add(delta));
        push(base_minor.saturating_sub(delta));
    }

    values.sort_by_key(|value| (i32::from(value.saturating_sub(center_minor).abs()), *value));
    values
}

fn path_clear(points: &[DagPoint], obstacles: &[Rect]) -> bool {
    points
        .windows(2)
        .all(|pair| segment_clear(pair[0], pair[1], obstacles))
}

fn segment_clear(a: DagPoint, b: DagPoint, obstacles: &[Rect]) -> bool {
    if a == b {
        return true;
    }

    if a.x == b.x {
        !obstacles
            .iter()
            .copied()
            .any(|rect| rect_intersects_vertical_segment(rect, a.x, a.y, b.y))
    } else if a.y == b.y {
        !obstacles
            .iter()
            .copied()
            .any(|rect| rect_intersects_horizontal_segment(rect, a.y, a.x, b.x))
    } else {
        false
    }
}

fn rect_intersects_vertical_segment(rect: Rect, x: i16, y_a: i16, y_b: i16) -> bool {
    let (lo, hi) = ordered_pair(y_a, y_b);
    let r_left = rect.x;
    let r_right = rect.x.saturating_add(rect.w as i16);
    let r_top = rect.y;
    let r_bottom = rect.y.saturating_add(rect.h as i16);
    x >= r_left && x < r_right && hi > r_top && lo < r_bottom
}

fn rect_intersects_horizontal_segment(rect: Rect, y: i16, x_a: i16, x_b: i16) -> bool {
    let (lo, hi) = ordered_pair(x_a, x_b);
    let r_left = rect.x;
    let r_right = rect.x.saturating_add(rect.w as i16);
    let r_top = rect.y;
    let r_bottom = rect.y.saturating_add(rect.h as i16);
    y >= r_top && y < r_bottom && hi > r_left && lo < r_right
}

fn ordered_pair(a: i16, b: i16) -> (i16, i16) {
    if a <= b { (a, b) } else { (b, a) }
}

fn signed_midpoint(a: i16, b: i16) -> i16 {
    ((i32::from(a) + i32::from(b)) / 2).clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

fn path_overlap_costs(
    points: &[DagPoint],
    occupancy: &HashMap<(i16, i16), HashMap<DagNodeId, u16>>,
    source: &DagNodeId,
) -> (u32, u32) {
    let mut other_source = 0u32;
    let mut same_source = 0u32;
    for sources in path_cells(points)
        .into_iter()
        .filter_map(|cell| occupancy.get(&cell))
    {
        for (id, count) in sources {
            if id.as_ref() == source.as_ref() {
                same_source = same_source.saturating_add(u32::from(*count));
            } else {
                other_source = other_source.saturating_add(u32::from(*count));
            }
        }
    }
    (other_source, same_source)
}

fn mark_route_occupancy(
    points: &[DagPoint],
    source: &DagNodeId,
    occupancy: &mut HashMap<(i16, i16), HashMap<DagNodeId, u16>>,
) {
    for cell in path_cells(points) {
        let by_source = occupancy.entry(cell).or_default();
        let entry = by_source.entry(source.clone()).or_default();
        *entry = entry.saturating_add(1);
    }
}

fn path_cells(points: &[DagPoint]) -> Vec<(i16, i16)> {
    let mut cells = Vec::new();
    for pair in points.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if a.x == b.x {
            let (start, end) = ordered_pair(a.y, b.y);
            for y in start..=end {
                if (a.x, y) != (points[0].x, points[0].y)
                    && (a.x, y) != (points[points.len() - 1].x, points[points.len() - 1].y)
                {
                    cells.push((a.x, y));
                }
            }
        } else if a.y == b.y {
            let (start, end) = ordered_pair(a.x, b.x);
            for x in start..=end {
                if (x, a.y) != (points[0].x, points[0].y)
                    && (x, a.y) != (points[points.len() - 1].x, points[points.len() - 1].y)
                {
                    cells.push((x, a.y));
                }
            }
        }
    }
    cells
}

fn compact_points(points: Vec<DagPoint>) -> Vec<DagPoint> {
    let mut compacted = Vec::with_capacity(points.len());
    for point in points {
        if compacted.last().copied() != Some(point) {
            compacted.push(point);
        }
    }
    compacted
}
