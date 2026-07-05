use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::style::Rect;
use crate::widgets::common::box_glyphs::{ALL_DIRECTIONS, EAST, NORTH, SOUTH, WEST};

use super::node::{
    EdgeCell, FlowchartRenderOutput, PositionedEdge, PositionedNode, PositionedSubgraph,
};
use super::{EdgeStyle, FlowDirection, Flowchart, NodeId, NodeShape};

#[derive(Clone, Debug)]
struct LayoutNode {
    spec_index: usize,
    layer: usize,
    order: usize,
    width: u16,
    height: u16,
    label_lines: Arc<[Arc<str>]>,
}

#[derive(Clone, Debug)]
struct LayoutEdge {
    index: usize,
    from: usize,
    to: usize,
    reversed: bool,
}

/// Measure a flowchart including outer chrome.
pub fn measure_flowchart(flowchart: &Flowchart) -> (u16, u16) {
    let output = build_flowchart_output(flowchart);
    let mut w = output.width.saturating_add(flowchart.padding.horizontal());
    let mut h = output.height.saturating_add(flowchart.padding.vertical());
    if flowchart.border {
        w = w.saturating_add(2);
        h = h.saturating_add(2);
    }
    (w, h)
}

pub(crate) fn build_flowchart_output(flowchart: &Flowchart) -> FlowchartRenderOutput {
    if flowchart.nodes.is_empty() {
        return FlowchartRenderOutput::default();
    }

    let id_to_index: HashMap<NodeId, usize> = flowchart
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), index))
        .collect();
    let mut layout_nodes: Vec<LayoutNode> = flowchart
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let (width, height, label_lines) =
                intrinsic_node_size(node.label.as_ref(), node.shape, flowchart.max_node_width);
            LayoutNode {
                spec_index: index,
                layer: 0,
                order: index,
                width,
                height,
                label_lines,
            }
        })
        .collect();

    let (layout_edges, back_edges) = classify_edges(flowchart, &id_to_index);
    assign_layers(&mut layout_nodes, &layout_edges);
    reduce_crossings(&mut layout_nodes, &layout_edges);
    let positioned_nodes = assign_coordinates(flowchart, &layout_nodes);
    let positioned_subgraphs = layout_subgraphs(flowchart, &positioned_nodes);
    let positioned_edges = route_edges(
        flowchart,
        &positioned_nodes,
        &positioned_subgraphs,
        &layout_edges,
        &back_edges,
    );

    normalize_output(positioned_nodes, positioned_subgraphs, positioned_edges)
}

fn classify_edges(
    flowchart: &Flowchart,
    id_to_index: &HashMap<NodeId, usize>,
) -> (Vec<LayoutEdge>, HashSet<usize>) {
    let mut adjacency = vec![Vec::<(usize, usize)>::new(); flowchart.nodes.len()];
    let mut raw_edges = Vec::new();
    for (edge_index, edge) in flowchart.edges.iter().enumerate() {
        let (Some(&from), Some(&to)) = (id_to_index.get(&edge.from), id_to_index.get(&edge.to))
        else {
            continue;
        };
        raw_edges.push((edge_index, from, to));
        adjacency[from].push((to, edge_index));
    }

    let mut visiting = vec![false; flowchart.nodes.len()];
    let mut visited = vec![false; flowchart.nodes.len()];
    let mut back_edges = HashSet::new();
    for node in 0..flowchart.nodes.len() {
        detect_back_edges(
            node,
            &adjacency,
            &mut visiting,
            &mut visited,
            &mut back_edges,
        );
    }

    let layout_edges = raw_edges
        .into_iter()
        .map(|(index, from, to)| LayoutEdge {
            index,
            from,
            to,
            reversed: back_edges.contains(&index),
        })
        .collect();
    (layout_edges, back_edges)
}

fn detect_back_edges(
    node: usize,
    adjacency: &[Vec<(usize, usize)>],
    visiting: &mut [bool],
    visited: &mut [bool],
    back_edges: &mut HashSet<usize>,
) {
    if visited[node] {
        return;
    }
    visiting[node] = true;
    for &(next, edge_index) in &adjacency[node] {
        if visiting[next] {
            back_edges.insert(edge_index);
        } else if !visited[next] {
            detect_back_edges(next, adjacency, visiting, visited, back_edges);
        }
    }
    visiting[node] = false;
    visited[node] = true;
}

fn assign_layers(nodes: &mut [LayoutNode], edges: &[LayoutEdge]) {
    let mut changed = true;
    let mut guard = 0usize;
    while changed && guard < nodes.len().saturating_mul(edges.len().max(1)).max(1) {
        changed = false;
        guard = guard.saturating_add(1);
        for edge in edges.iter().filter(|edge| !edge.reversed) {
            let next = nodes[edge.from].layer.saturating_add(1);
            if nodes[edge.to].layer < next {
                nodes[edge.to].layer = next;
                changed = true;
            }
        }
    }

    pull_predecessorless_sources_forward(nodes, edges);
}

fn pull_predecessorless_sources_forward(nodes: &mut [LayoutNode], edges: &[LayoutEdge]) {
    let mut has_incoming = vec![false; nodes.len()];
    for edge in edges.iter().filter(|edge| !edge.reversed) {
        has_incoming[edge.to] = true;
    }

    for index in 0..nodes.len() {
        if has_incoming[index] {
            continue;
        }

        let Some(target_layer) = edges
            .iter()
            .filter(|edge| !edge.reversed && edge.from == index)
            .map(|edge| nodes[edge.to].layer.saturating_sub(1))
            .min()
        else {
            continue;
        };

        if target_layer > nodes[index].layer {
            nodes[index].layer = target_layer;
        }
    }
}

fn reduce_crossings(nodes: &mut [LayoutNode], edges: &[LayoutEdge]) {
    let max_layer = nodes.iter().map(|node| node.layer).max().unwrap_or(0);
    for layer in 0..=max_layer {
        let mut indices: Vec<_> = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| (node.layer == layer).then_some(index))
            .collect();
        indices.sort_by_key(|&index| nodes[index].order);
        for (order, index) in indices.into_iter().enumerate() {
            nodes[index].order = order;
        }
    }

    for sweep in 0..12 {
        let mut changed = false;
        let layers: Box<dyn Iterator<Item = usize>> = if sweep % 2 == 0 {
            Box::new(1..=max_layer)
        } else {
            Box::new((0..max_layer).rev())
        };
        for layer in layers {
            let mut layer_nodes: Vec<_> = nodes
                .iter()
                .enumerate()
                .filter_map(|(index, node)| (node.layer == layer).then_some(index))
                .collect();
            layer_nodes.sort_by_key(|&index| nodes[index].order);
            let previous = layer_nodes.clone();
            layer_nodes.sort_by(|&a, &b| {
                let ba = barycenter(a, nodes, edges, sweep % 2 == 0);
                let bb = barycenter(b, nodes, edges, sweep % 2 == 0);
                ba.partial_cmp(&bb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| nodes[a].order.cmp(&nodes[b].order))
            });
            for (order, index) in layer_nodes.iter().copied().enumerate() {
                if nodes[index].order != order {
                    changed = true;
                }
                nodes[index].order = order;
            }
            if layer_nodes != previous {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

fn barycenter(index: usize, nodes: &[LayoutNode], edges: &[LayoutEdge], predecessors: bool) -> f32 {
    let mut total = 0f32;
    let mut count = 0f32;
    for edge in edges {
        let (from, to) = if edge.reversed {
            (edge.to, edge.from)
        } else {
            (edge.from, edge.to)
        };
        let neighbor = if predecessors && to == index {
            Some(from)
        } else if !predecessors && from == index {
            Some(to)
        } else {
            None
        };
        if let Some(neighbor) = neighbor {
            total += nodes[neighbor].order as f32;
            count += 1.0;
        }
    }
    if count == 0.0 {
        nodes[index].order as f32
    } else {
        total / count
    }
}

fn assign_coordinates(flowchart: &Flowchart, layout_nodes: &[LayoutNode]) -> Vec<PositionedNode> {
    let max_layer = layout_nodes
        .iter()
        .map(|node| node.layer)
        .max()
        .unwrap_or(0);
    let mut layer_major_offsets = vec![0i32; max_layer.saturating_add(1)];
    let mut cursor = 0i32;
    for (layer, offset) in layer_major_offsets.iter_mut().enumerate() {
        *offset = cursor;
        let layer_extent = layout_nodes
            .iter()
            .filter(|node| node.layer == layer)
            .map(|node| match flowchart.direction {
                FlowDirection::TopDown | FlowDirection::BottomUp => node.height,
                FlowDirection::LeftRight | FlowDirection::RightLeft => node.width,
            })
            .max()
            .unwrap_or(0);
        cursor = cursor
            .saturating_add(i32::from(layer_extent))
            .saturating_add(i32::from(flowchart.layer_gap));
    }

    let max_layer_extent = cursor.saturating_sub(i32::from(flowchart.layer_gap));
    let (node_offsets, total_minor) = compute_node_offsets(flowchart, layout_nodes, max_layer);
    let mut positioned = Vec::with_capacity(layout_nodes.len());
    for (index, node) in layout_nodes.iter().enumerate() {
        let spec = &flowchart.nodes[node.spec_index];
        let minor = node_offsets[index].saturating_sub(total_minor / 2);
        let layer_offset = match flowchart.direction {
            FlowDirection::TopDown | FlowDirection::LeftRight => layer_major_offsets[node.layer],
            FlowDirection::BottomUp | FlowDirection::RightLeft => {
                max_layer_extent.saturating_sub(layer_major_offsets[node.layer])
            }
        };
        let (x, y) = match flowchart.direction {
            FlowDirection::TopDown | FlowDirection::BottomUp => (minor, layer_offset),
            FlowDirection::LeftRight | FlowDirection::RightLeft => (layer_offset, minor),
        };
        positioned.push(PositionedNode {
            rect: Rect {
                x: clamp_i32_to_i16(x),
                y: clamp_i32_to_i16(y),
                w: node.width,
                h: node.height,
            },
            id: spec.id.clone(),
            label: spec.label.clone(),
            label_lines: node.label_lines.clone(),
            shape: spec.shape,
            style: flowchart
                .class_assignments
                .get(&spec.id)
                .and_then(|class| flowchart.class_defs.get(class))
                .copied()
                .unwrap_or_default()
                .patch(spec.style),
            hover_style: spec.hover_style,
        });
    }
    positioned
}

/// Recursively place nodes into hierarchical sub-lanes so that direct children of a container
/// occupy a sub-lane separate from each child subgraph's sub-lane. Ensures non-member nodes
/// can never share x-range with a subgraph rect.
fn compute_node_offsets(
    flowchart: &Flowchart,
    layout_nodes: &[LayoutNode],
    max_layer: usize,
) -> (Vec<i32>, i32) {
    let mut offsets = vec![0i32; layout_nodes.len()];
    let total = place_container(flowchart, layout_nodes, max_layer, None, 0, &mut offsets);
    (offsets, total)
}

fn place_container(
    flowchart: &Flowchart,
    layout_nodes: &[LayoutNode],
    max_layer: usize,
    container: Option<&NodeId>,
    base: i32,
    offsets: &mut [i32],
) -> i32 {
    let direct_indices: Vec<usize> = layout_nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            (flowchart.nodes[node.spec_index].parent.as_ref() == container).then_some(index)
        })
        .collect();
    let child_subgraph_ids: Vec<NodeId> = flowchart
        .subgraphs
        .iter()
        .filter(|sg| sg.parent.as_ref() == container)
        .map(|sg| sg.id.clone())
        .collect();

    let mut cursor = base;
    let mut placed_anything = false;

    if !direct_indices.is_empty() {
        let mut sub_lane_extent = 0i32;
        for layer in 0..=max_layer {
            let group = layered_group(layout_nodes, &direct_indices, layer);
            sub_lane_extent =
                sub_lane_extent.max(minor_extent_for_indices(flowchart, layout_nodes, &group));
        }
        for layer in 0..=max_layer {
            let group = layered_group(layout_nodes, &direct_indices, layer);
            if group.is_empty() {
                continue;
            }
            let group_extent = minor_extent_for_indices(flowchart, layout_nodes, &group);
            let centering = (sub_lane_extent.saturating_sub(group_extent)) / 2;
            let mut local = 0i32;
            for index in group {
                offsets[index] = cursor.saturating_add(centering).saturating_add(local);
                let extent = match flowchart.direction {
                    FlowDirection::TopDown | FlowDirection::BottomUp => layout_nodes[index].width,
                    _ => layout_nodes[index].height,
                };
                local = local
                    .saturating_add(i32::from(extent))
                    .saturating_add(i32::from(flowchart.node_gap));
            }
        }
        cursor = cursor.saturating_add(sub_lane_extent);
        placed_anything = true;
    }

    for sg_id in &child_subgraph_ids {
        if placed_anything {
            cursor = cursor.saturating_add(cluster_gap(flowchart));
        }
        let inner_start = cursor;
        let sub_end = place_container(
            flowchart,
            layout_nodes,
            max_layer,
            Some(sg_id),
            inner_start,
            offsets,
        );
        let inner_extent = sub_end.saturating_sub(inner_start);
        let header_floor = subgraph_header_floor(flowchart, sg_id);
        let needed = inner_extent.max(header_floor);
        cursor = inner_start.saturating_add(needed);
        placed_anything = true;
    }

    cursor
}

fn layered_group(layout_nodes: &[LayoutNode], pool: &[usize], layer: usize) -> Vec<usize> {
    let mut group: Vec<usize> = pool
        .iter()
        .copied()
        .filter(|&idx| layout_nodes[idx].layer == layer)
        .collect();
    group.sort_by_key(|&idx| layout_nodes[idx].order);
    group
}

fn subgraph_header_floor(flowchart: &Flowchart, id: &NodeId) -> i32 {
    flowchart
        .subgraphs
        .iter()
        .find(|sg| &sg.id == id)
        .map(|sg| {
            // Header label width minus padding/border the chrome calc adds back later.
            sg.label
                .chars()
                .count()
                .saturating_add(2)
                .saturating_sub(usize::from(flowchart.subgraph_padding.horizontal()))
                .saturating_sub(2) as i32
        })
        .unwrap_or(0)
}

fn cluster_gap(flowchart: &Flowchart) -> i32 {
    i32::from(flowchart.node_gap)
        .saturating_add(i32::from(flowchart.subgraph_padding.horizontal()))
        .saturating_add(4)
}

fn minor_extent_for_indices(
    flowchart: &Flowchart,
    layout_nodes: &[LayoutNode],
    indices: &[usize],
) -> i32 {
    indices
        .iter()
        .map(|&index| match flowchart.direction {
            FlowDirection::TopDown | FlowDirection::BottomUp => layout_nodes[index].width,
            FlowDirection::LeftRight | FlowDirection::RightLeft => layout_nodes[index].height,
        } as i32)
        .sum::<i32>()
        .saturating_add(
            i32::from(flowchart.node_gap).saturating_mul(indices.len().saturating_sub(1) as i32),
        )
}

fn layout_subgraphs(flowchart: &Flowchart, nodes: &[PositionedNode]) -> Vec<PositionedSubgraph> {
    let node_rects: HashMap<_, _> = nodes
        .iter()
        .map(|node| (node.id.clone(), node.rect))
        .collect();
    let mut out = Vec::new();
    for subgraph in flowchart.subgraphs.iter().rev() {
        let mut min_x = i16::MAX;
        let mut min_y = i16::MAX;
        let mut max_x = i16::MIN;
        let mut max_y = i16::MIN;
        for node in flowchart
            .nodes
            .iter()
            .filter(|node| node.parent.as_ref() == Some(&subgraph.id))
        {
            if let Some(rect) = node_rects.get(&node.id) {
                include_rect(*rect, &mut min_x, &mut min_y, &mut max_x, &mut max_y);
            }
        }
        for nested in out.iter().filter(|nested: &&PositionedSubgraph| {
            flowchart
                .subgraphs
                .iter()
                .find(|candidate| candidate.id == nested.id)
                .and_then(|candidate| candidate.parent.as_ref())
                == Some(&subgraph.id)
        }) {
            include_rect(nested.rect, &mut min_x, &mut min_y, &mut max_x, &mut max_y);
        }
        if min_x == i16::MAX {
            continue;
        }
        let pad = flowchart.subgraph_padding;
        let header_width = subgraph
            .label
            .chars()
            .count()
            .saturating_add(2)
            .min(u16::MAX as usize) as u16;
        let rect = Rect {
            x: min_x.saturating_sub(pad.left as i16).saturating_sub(1),
            y: min_y.saturating_sub(pad.top as i16).saturating_sub(2),
            w: (i32::from(max_x) - i32::from(min_x))
                .max(i32::from(header_width))
                .saturating_add(i32::from(pad.horizontal()))
                .saturating_add(2)
                .clamp(0, u16::MAX as i32) as u16,
            h: (i32::from(max_y) - i32::from(min_y))
                .saturating_add(i32::from(pad.vertical()))
                .saturating_add(3)
                .clamp(0, u16::MAX as i32) as u16,
        };
        let depth = subgraph_depth(flowchart, &subgraph.id);
        out.push(PositionedSubgraph {
            header_rect: Rect {
                x: rect.x.saturating_add(1),
                y: rect.y,
                w: header_width.min(rect.w.saturating_sub(2)),
                h: 1,
            },
            rect,
            id: subgraph.id.clone(),
            label: subgraph.label.clone(),
            depth,
            style: flowchart
                .class_assignments
                .get(&subgraph.id)
                .and_then(|class| flowchart.class_defs.get(class))
                .copied()
                .unwrap_or_default()
                .patch(subgraph.style),
        });
    }
    out.sort_by_key(|subgraph| subgraph.depth);
    out
}

fn route_edges(
    flowchart: &Flowchart,
    nodes: &[PositionedNode],
    subgraphs: &[PositionedSubgraph],
    layout_edges: &[LayoutEdge],
    back_edges: &HashSet<usize>,
) -> Vec<PositionedEdge> {
    let by_id: HashMap<_, _> = nodes
        .iter()
        .map(|node| (node.id.clone(), node.rect))
        .collect();
    let diagram_bounds = bounds_for_rects(
        nodes
            .iter()
            .map(|node| node.rect)
            .chain(subgraphs.iter().map(|subgraph| subgraph.rect)),
    );
    layout_edges
        .iter()
        .filter_map(|layout_edge| {
            let edge = flowchart.edges.get(layout_edge.index)?;
            let from = *by_id.get(&edge.from)?;
            let to = *by_id.get(&edge.to)?;
            let obstacles = obstacle_rects(flowchart, nodes, subgraphs, &edge.from, &edge.to);
            let (cells, label_pos, from_head, to_head) = route_edge_cells(
                from,
                to,
                flowchart.direction,
                EdgeRouteOptions {
                    style: edge.style,
                    label: edge.label.as_deref(),
                    is_back_edge: back_edges.contains(&layout_edge.index),
                    diagram_bounds,
                    obstacles: &obstacles,
                },
            );
            Some(PositionedEdge {
                index: layout_edge.index,
                from: edge.from.clone(),
                to: edge.to.clone(),
                label: edge.label.clone(),
                style: edge.style,
                head_from: edge.head_from,
                head_to: edge.head_to,
                line_style: edge.line_style,
                label_style: edge.label_style,
                cells,
                label_pos,
                head_from_pos: from_head,
                head_to_pos: to_head,
            })
        })
        .collect()
}

fn obstacle_rects(
    flowchart: &Flowchart,
    nodes: &[PositionedNode],
    subgraphs: &[PositionedSubgraph],
    from: &NodeId,
    to: &NodeId,
) -> Vec<Rect> {
    let mut out = Vec::new();
    for node in nodes {
        if &node.id != from && &node.id != to {
            out.push(node.rect);
        }
    }
    for subgraph in subgraphs {
        if !subgraph_contains_node(flowchart, &subgraph.id, from)
            && !subgraph_contains_node(flowchart, &subgraph.id, to)
        {
            out.push(subgraph.rect);
        }
    }
    out
}

fn subgraph_contains_node(flowchart: &Flowchart, subgraph_id: &NodeId, node_id: &NodeId) -> bool {
    let Some(node) = flowchart.nodes.iter().find(|node| &node.id == node_id) else {
        return false;
    };
    let mut current = node.parent.clone();
    while let Some(parent) = current {
        if &parent == subgraph_id {
            return true;
        }
        current = flowchart
            .subgraphs
            .iter()
            .find(|candidate| candidate.id == parent)
            .and_then(|candidate| candidate.parent.clone());
    }
    false
}

type RoutedEdge = (
    Vec<EdgeCell>,
    Option<(i16, i16)>,
    Option<(i16, i16, FlowDirection)>,
    Option<(i16, i16, FlowDirection)>,
);

#[derive(Clone, Copy, Debug)]
struct EdgeRouteOptions<'a> {
    style: EdgeStyle,
    label: Option<&'a str>,
    is_back_edge: bool,
    diagram_bounds: Option<Rect>,
    obstacles: &'a [Rect],
}

fn route_edge_cells(
    from: Rect,
    to: Rect,
    direction: FlowDirection,
    options: EdgeRouteOptions<'_>,
) -> RoutedEdge {
    if matches!(options.style, EdgeStyle::Invisible) {
        return (Vec::new(), None, None, None);
    }
    let (start, end) = ports(from, to, direction);
    let points = if options.is_back_edge {
        back_edge_points(start, end, direction, &options)
    } else {
        forward_edge_points(start, end, direction, options.obstacles)
    };

    let mut edge_bits = HashMap::<(i16, i16), u8>::new();
    let (source_attachment, target_attachment) = port_attachment_bits(direction);
    *edge_bits.entry(start).or_insert(0) |= source_attachment;
    *edge_bits.entry(end).or_insert(0) |= target_attachment;
    for pair in points.windows(2) {
        connect(pair[0], pair[1], &mut edge_bits);
    }
    let mut cells: Vec<_> = edge_bits
        .into_iter()
        .filter_map(|((x, y), bits)| {
            let bits = bits & ALL_DIRECTIONS;
            (bits != 0).then_some(EdgeCell { x, y, bits })
        })
        .collect();
    cells.sort_by_key(|cell| (cell.y, cell.x));
    let label_pos = options.label.and_then(|label| {
        let label_width = label.chars().count() as i16;
        let segment_mid = if options.is_back_edge {
            back_edge_label_segment_midpoint(&points, direction)
                .or_else(|| longest_segment_midpoint(&points))
        } else {
            longest_segment_midpoint(&points)
        };
        segment_mid.map(|(x, y)| (x.saturating_sub(label_width / 2), y))
    });
    let from_head = Some((start.0, start.1, opposite(direction)));
    let to_head = Some((end.0, end.1, direction));
    (cells, label_pos, from_head, to_head)
}

fn forward_edge_points(
    start: (i16, i16),
    end: (i16, i16),
    direction: FlowDirection,
    obstacles: &[Rect],
) -> Vec<(i16, i16)> {
    let mut points = vec![start];
    let vertical_axis = matches!(direction, FlowDirection::TopDown | FlowDirection::BottomUp);
    let (start_axis, end_axis, start_minor, end_minor) = if vertical_axis {
        (start.1, end.1, start.0, end.0)
    } else {
        (start.0, end.0, start.1, end.1)
    };

    let collinear = start_minor == end_minor;
    let blocked_collinear = collinear
        && obstacles.iter().any(|rect| {
            if vertical_axis {
                rect_intersects_vertical_segment(*rect, start_minor, start_axis, end_axis)
            } else {
                rect_intersects_horizontal_segment(*rect, start_minor, start_axis, end_axis)
            }
        });

    if blocked_collinear {
        let detour_minor = pick_detour_minor(start, end, direction, obstacles);
        if vertical_axis {
            points.push((detour_minor, start.1));
            points.push((detour_minor, end.1));
        } else {
            points.push((start.0, detour_minor));
            points.push((end.0, detour_minor));
        }
    } else {
        let gutter = choose_gutter_axis(
            start_axis,
            end_axis,
            start_minor,
            end_minor,
            obstacles,
            vertical_axis,
        );
        if vertical_axis {
            points.push((start.0, gutter));
            points.push((end.0, gutter));
        } else {
            points.push((gutter, start.1));
            points.push((gutter, end.1));
        }
    }
    points.push(end);
    points
}

fn back_edge_points(
    start: (i16, i16),
    end: (i16, i16),
    direction: FlowDirection,
    options: &EdgeRouteOptions<'_>,
) -> Vec<(i16, i16)> {
    let mut points = vec![start];
    match direction {
        FlowDirection::TopDown | FlowDirection::BottomUp => {
            let side_x = pick_back_edge_side_x(start, end, options);
            let initial_approach = match direction {
                FlowDirection::TopDown => end.1.saturating_sub(1),
                _ => end.1.saturating_add(1),
            };
            let approach_y = push_horizontal_axis_outside_obstacles(
                initial_approach,
                side_x,
                end.0,
                options.obstacles,
                direction,
                true,
            );
            let exit_y = push_horizontal_axis_outside_obstacles(
                start.1,
                start.0,
                side_x,
                options.obstacles,
                direction,
                false,
            );
            points.push((start.0, exit_y));
            points.push((side_x, exit_y));
            points.push((side_x, approach_y));
            points.push((end.0, approach_y));
        }
        FlowDirection::LeftRight | FlowDirection::RightLeft => {
            let side_y = pick_back_edge_side_y(start, end, options);
            let initial_approach = match direction {
                FlowDirection::LeftRight => end.0.saturating_sub(1),
                _ => end.0.saturating_add(1),
            };
            let approach_x = push_vertical_axis_outside_obstacles(
                initial_approach,
                side_y,
                end.1,
                options.obstacles,
                direction,
                true,
            );
            let exit_x = push_vertical_axis_outside_obstacles(
                start.0,
                start.1,
                side_y,
                options.obstacles,
                direction,
                false,
            );
            points.push((exit_x, start.1));
            points.push((exit_x, side_y));
            points.push((approach_x, side_y));
            points.push((approach_x, end.1));
        }
    }
    points.push(end);
    points
}

/// Push a horizontal segment's y away from obstacles. For TD back-edges, the approach
/// horizontal (toward_target=true) lives near the target at top of the diagram and must
/// rise *above* every non-containing subgraph that extends over the target. The exit
/// horizontal (toward_target=false) lives at the source's bottom and must drop *below*
/// any obstacle that extends past it.
fn push_horizontal_axis_outside_obstacles(
    initial_y: i16,
    x_a: i16,
    x_b: i16,
    obstacles: &[Rect],
    direction: FlowDirection,
    toward_target: bool,
) -> i16 {
    let push_up = match (direction, toward_target) {
        (FlowDirection::TopDown, true) => true,
        (FlowDirection::TopDown, false) => false,
        (FlowDirection::BottomUp, true) => false,
        (FlowDirection::BottomUp, false) => true,
        _ => return initial_y,
    };
    let mut candidate = initial_y;
    for _ in 0..16 {
        let blocking: Vec<Rect> = obstacles
            .iter()
            .copied()
            .filter(|rect| rect_intersects_horizontal_segment(*rect, candidate, x_a, x_b))
            .collect();
        if blocking.is_empty() {
            return candidate;
        }
        let next = if push_up {
            blocking.iter().map(|rect| rect.y.saturating_sub(1)).min()
        } else {
            blocking
                .iter()
                .map(|rect| rect.y.saturating_add(rect.h as i16))
                .max()
        };
        match next {
            Some(n) if n != candidate => candidate = n,
            _ => return initial_y,
        }
    }
    initial_y
}

fn push_vertical_axis_outside_obstacles(
    initial_x: i16,
    y_a: i16,
    y_b: i16,
    obstacles: &[Rect],
    direction: FlowDirection,
    toward_target: bool,
) -> i16 {
    let push_left = match (direction, toward_target) {
        (FlowDirection::LeftRight, true) => true,
        (FlowDirection::LeftRight, false) => false,
        (FlowDirection::RightLeft, true) => false,
        (FlowDirection::RightLeft, false) => true,
        _ => return initial_x,
    };
    let mut candidate = initial_x;
    for _ in 0..16 {
        let blocking: Vec<Rect> = obstacles
            .iter()
            .copied()
            .filter(|rect| rect_intersects_vertical_segment(*rect, candidate, y_a, y_b))
            .collect();
        if blocking.is_empty() {
            return candidate;
        }
        let next = if push_left {
            blocking.iter().map(|rect| rect.x.saturating_sub(1)).min()
        } else {
            blocking
                .iter()
                .map(|rect| rect.x.saturating_add(rect.w as i16))
                .max()
        };
        match next {
            Some(n) if n != candidate => candidate = n,
            _ => return initial_x,
        }
    }
    initial_x
}

fn choose_gutter_axis(
    start_axis: i16,
    end_axis: i16,
    start_minor: i16,
    end_minor: i16,
    obstacles: &[Rect],
    is_y_axis: bool,
) -> i16 {
    let preferred = signed_midpoint(start_axis, end_axis);
    let just_before_end = if start_axis <= end_axis {
        end_axis.saturating_sub(1).max(start_axis.saturating_add(1))
    } else {
        end_axis.saturating_add(1).min(start_axis.saturating_sub(1))
    };
    let just_after_start = if start_axis <= end_axis {
        start_axis.saturating_add(1).min(end_axis.saturating_sub(1))
    } else {
        start_axis.saturating_sub(1).max(end_axis.saturating_add(1))
    };
    let candidates = [preferred, just_before_end, just_after_start];
    for candidate in candidates {
        if !horizontal_or_vertical_crosses(start_minor, end_minor, candidate, obstacles, is_y_axis)
        {
            return candidate;
        }
    }
    preferred
}

fn horizontal_or_vertical_crosses(
    minor_a: i16,
    minor_b: i16,
    axis_value: i16,
    obstacles: &[Rect],
    is_y_axis: bool,
) -> bool {
    let (lo, hi) = if minor_a <= minor_b {
        (minor_a, minor_b)
    } else {
        (minor_b, minor_a)
    };
    obstacles.iter().any(|rect| {
        let r_left = rect.x;
        let r_right = rect.x.saturating_add(rect.w as i16);
        let r_top = rect.y;
        let r_bottom = rect.y.saturating_add(rect.h as i16);
        if is_y_axis {
            axis_value >= r_top && axis_value < r_bottom && hi > r_left && lo < r_right
        } else {
            axis_value >= r_left && axis_value < r_right && hi > r_top && lo < r_bottom
        }
    })
}

fn rect_intersects_vertical_segment(rect: Rect, x: i16, y_a: i16, y_b: i16) -> bool {
    let (lo, hi) = if y_a <= y_b { (y_a, y_b) } else { (y_b, y_a) };
    let r_left = rect.x;
    let r_right = rect.x.saturating_add(rect.w as i16);
    let r_top = rect.y;
    let r_bottom = rect.y.saturating_add(rect.h as i16);
    x >= r_left && x < r_right && hi > r_top && lo < r_bottom
}

fn rect_intersects_horizontal_segment(rect: Rect, y: i16, x_a: i16, x_b: i16) -> bool {
    let (lo, hi) = if x_a <= x_b { (x_a, x_b) } else { (x_b, x_a) };
    let r_left = rect.x;
    let r_right = rect.x.saturating_add(rect.w as i16);
    let r_top = rect.y;
    let r_bottom = rect.y.saturating_add(rect.h as i16);
    y >= r_top && y < r_bottom && hi > r_left && lo < r_right
}

fn pick_detour_minor(
    start: (i16, i16),
    end: (i16, i16),
    direction: FlowDirection,
    obstacles: &[Rect],
) -> i16 {
    let vertical_axis = matches!(direction, FlowDirection::TopDown | FlowDirection::BottomUp);
    let (axis_lo, axis_hi) = if vertical_axis {
        let (a, b) = (start.1, end.1);
        if a <= b { (a, b) } else { (b, a) }
    } else {
        let (a, b) = (start.0, end.0);
        if a <= b { (a, b) } else { (b, a) }
    };
    let blocking: Vec<Rect> = obstacles
        .iter()
        .copied()
        .filter(|rect| {
            if vertical_axis {
                rect.y.saturating_add(rect.h as i16) > axis_lo && rect.y < axis_hi
            } else {
                rect.x.saturating_add(rect.w as i16) > axis_lo && rect.x < axis_hi
            }
        })
        .collect();
    let center_minor = if vertical_axis { start.0 } else { start.1 };
    let right_edge = blocking
        .iter()
        .map(|rect| {
            if vertical_axis {
                rect.x.saturating_add(rect.w as i16)
            } else {
                rect.y.saturating_add(rect.h as i16)
            }
        })
        .max()
        .unwrap_or(center_minor);
    let left_edge = blocking
        .iter()
        .map(|rect| if vertical_axis { rect.x } else { rect.y })
        .min()
        .unwrap_or(center_minor);
    let go_right = (right_edge.saturating_add(1))
        .saturating_sub(center_minor)
        .abs()
        <= center_minor
            .saturating_sub(left_edge.saturating_sub(1))
            .abs();
    if go_right {
        right_edge.saturating_add(1)
    } else {
        left_edge.saturating_sub(1)
    }
}

fn pick_back_edge_side_x(
    start: (i16, i16),
    end: (i16, i16),
    options: &EdgeRouteOptions<'_>,
) -> i16 {
    let Some(bounds) = options.diagram_bounds else {
        return start.0.min(end.0).saturating_sub(2);
    };
    let center = signed_midpoint(start.0, end.0);
    let bounds_right = bounds.x.saturating_add(bounds.w as i16);
    let left_dist = i32::from(center) - i32::from(bounds.x);
    let right_dist = i32::from(bounds_right) - i32::from(center);
    if right_dist >= left_dist {
        bounds_right.saturating_add(2)
    } else {
        bounds.x.saturating_sub(2)
    }
}

fn pick_back_edge_side_y(
    start: (i16, i16),
    end: (i16, i16),
    options: &EdgeRouteOptions<'_>,
) -> i16 {
    let Some(bounds) = options.diagram_bounds else {
        return start.1.min(end.1).saturating_sub(2);
    };
    let center = signed_midpoint(start.1, end.1);
    let bounds_bottom = bounds.y.saturating_add(bounds.h as i16);
    let top_dist = i32::from(center) - i32::from(bounds.y);
    let bottom_dist = i32::from(bounds_bottom) - i32::from(center);
    if bottom_dist >= top_dist {
        bounds_bottom.saturating_add(2)
    } else {
        bounds.y.saturating_sub(2)
    }
}

fn back_edge_label_segment_midpoint(
    points: &[(i16, i16)],
    direction: FlowDirection,
) -> Option<(i16, i16)> {
    // Back-edge layout produces 6 points:
    // [start, exit_corner_a, exit_corner_b, approach_corner_a, approach_corner_b, end]
    // The approach segment (closest to the target) is points[3]→points[4].
    if points.len() < 6 {
        return None;
    }
    let approach = (points[3], points[4]);
    match direction {
        FlowDirection::TopDown | FlowDirection::BottomUp => {
            let mid_x = ((i32::from(approach.0.0) + i32::from(approach.1.0)) / 2) as i16;
            let label_y = match direction {
                FlowDirection::TopDown => approach.0.1.saturating_sub(1),
                _ => approach.0.1.saturating_add(1),
            };
            Some((mid_x, label_y))
        }
        FlowDirection::LeftRight | FlowDirection::RightLeft => {
            let mid_y = ((i32::from(approach.0.1) + i32::from(approach.1.1)) / 2) as i16;
            let label_x = match direction {
                FlowDirection::LeftRight => approach.0.0.saturating_sub(1),
                _ => approach.0.0.saturating_add(1),
            };
            Some((label_x, mid_y))
        }
    }
}

fn signed_midpoint(a: i16, b: i16) -> i16 {
    ((i32::from(a) + i32::from(b)) / 2).clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

fn bounds_for_rects(rects: impl IntoIterator<Item = Rect>) -> Option<Rect> {
    let mut min_x = i16::MAX;
    let mut min_y = i16::MAX;
    let mut max_x = i16::MIN;
    let mut max_y = i16::MIN;
    for rect in rects {
        include_rect(rect, &mut min_x, &mut min_y, &mut max_x, &mut max_y);
    }
    (min_x != i16::MAX).then(|| Rect {
        x: min_x,
        y: min_y,
        w: (i32::from(max_x) - i32::from(min_x)).clamp(0, u16::MAX as i32) as u16,
        h: (i32::from(max_y) - i32::from(min_y)).clamp(0, u16::MAX as i32) as u16,
    })
}

fn ports(from: Rect, to: Rect, direction: FlowDirection) -> ((i16, i16), (i16, i16)) {
    match direction {
        FlowDirection::TopDown => (
            (
                from.x.saturating_add(from.w as i16 / 2),
                from.y.saturating_add(from.h as i16),
            ),
            (to.x.saturating_add(to.w as i16 / 2), to.y.saturating_sub(1)),
        ),
        FlowDirection::BottomUp => (
            (
                from.x.saturating_add(from.w as i16 / 2),
                from.y.saturating_sub(1),
            ),
            (
                to.x.saturating_add(to.w as i16 / 2),
                to.y.saturating_add(to.h as i16),
            ),
        ),
        FlowDirection::LeftRight => (
            (
                from.x.saturating_add(from.w as i16),
                from.y.saturating_add(from.h as i16 / 2),
            ),
            (to.x.saturating_sub(1), to.y.saturating_add(to.h as i16 / 2)),
        ),
        FlowDirection::RightLeft => (
            (
                from.x.saturating_sub(1),
                from.y.saturating_add(from.h as i16 / 2),
            ),
            (
                to.x.saturating_add(to.w as i16),
                to.y.saturating_add(to.h as i16 / 2),
            ),
        ),
    }
}

fn opposite(direction: FlowDirection) -> FlowDirection {
    match direction {
        FlowDirection::TopDown => FlowDirection::BottomUp,
        FlowDirection::BottomUp => FlowDirection::TopDown,
        FlowDirection::LeftRight => FlowDirection::RightLeft,
        FlowDirection::RightLeft => FlowDirection::LeftRight,
    }
}

fn port_attachment_bits(direction: FlowDirection) -> (u8, u8) {
    match direction {
        FlowDirection::TopDown => (NORTH, SOUTH),
        FlowDirection::BottomUp => (SOUTH, NORTH),
        FlowDirection::LeftRight => (WEST, EAST),
        FlowDirection::RightLeft => (EAST, WEST),
    }
}

fn connect(a: (i16, i16), b: (i16, i16), edge_bits: &mut HashMap<(i16, i16), u8>) {
    if a == b {
        return;
    }
    if a.0 == b.0 {
        let (start, end) = if a.1 <= b.1 { (a.1, b.1) } else { (b.1, a.1) };
        for y in start..=end {
            let mut bits = 0;
            if y > start {
                bits |= NORTH;
            }
            if y < end {
                bits |= SOUTH;
            }
            *edge_bits.entry((a.0, y)).or_insert(0) |= bits;
        }
    } else if a.1 == b.1 {
        let (start, end) = if a.0 <= b.0 { (a.0, b.0) } else { (b.0, a.0) };
        for x in start..=end {
            let mut bits = 0;
            if x > start {
                bits |= WEST;
            }
            if x < end {
                bits |= EAST;
            }
            *edge_bits.entry((x, a.1)).or_insert(0) |= bits;
        }
    }
}

fn longest_segment_midpoint(points: &[(i16, i16)]) -> Option<(i16, i16)> {
    points
        .windows(2)
        .max_by_key(|pair| pair[0].0.abs_diff(pair[1].0) + pair[0].1.abs_diff(pair[1].1))
        .map(|pair| ((pair[0].0 + pair[1].0) / 2, (pair[0].1 + pair[1].1) / 2))
}

fn normalize_output(
    mut nodes: Vec<PositionedNode>,
    mut subgraphs: Vec<PositionedSubgraph>,
    mut edges: Vec<PositionedEdge>,
) -> FlowchartRenderOutput {
    let mut min_x = i16::MAX;
    let mut min_y = i16::MAX;
    let mut max_x = i16::MIN;
    let mut max_y = i16::MIN;

    for node in &nodes {
        include_rect(node.rect, &mut min_x, &mut min_y, &mut max_x, &mut max_y);
    }
    for subgraph in &subgraphs {
        include_rect(
            subgraph.rect,
            &mut min_x,
            &mut min_y,
            &mut max_x,
            &mut max_y,
        );
    }
    for edge in &edges {
        for cell in &edge.cells {
            min_x = min_x.min(cell.x);
            min_y = min_y.min(cell.y);
            max_x = max_x.max(cell.x.saturating_add(1));
            max_y = max_y.max(cell.y.saturating_add(1));
        }
        if let (Some((x, y)), Some(label)) = (edge.label_pos, edge.label.as_ref()) {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x.saturating_add(label.chars().count() as i16));
            max_y = max_y.max(y.saturating_add(1));
        }
    }

    if min_x == i16::MAX || min_y == i16::MAX {
        return FlowchartRenderOutput::default();
    }

    let shift_x = 0i16.saturating_sub(min_x);
    let shift_y = 0i16.saturating_sub(min_y);
    for node in &mut nodes {
        node.rect.x = node.rect.x.saturating_add(shift_x);
        node.rect.y = node.rect.y.saturating_add(shift_y);
    }
    for subgraph in &mut subgraphs {
        subgraph.rect.x = subgraph.rect.x.saturating_add(shift_x);
        subgraph.rect.y = subgraph.rect.y.saturating_add(shift_y);
        subgraph.header_rect.x = subgraph.header_rect.x.saturating_add(shift_x);
        subgraph.header_rect.y = subgraph.header_rect.y.saturating_add(shift_y);
    }
    for edge in &mut edges {
        for cell in &mut edge.cells {
            cell.x = cell.x.saturating_add(shift_x);
            cell.y = cell.y.saturating_add(shift_y);
        }
        if let Some((x, y)) = &mut edge.label_pos {
            *x = x.saturating_add(shift_x);
            *y = y.saturating_add(shift_y);
        }
        if let Some((x, y, _)) = &mut edge.head_from_pos {
            *x = x.saturating_add(shift_x);
            *y = y.saturating_add(shift_y);
        }
        if let Some((x, y, _)) = &mut edge.head_to_pos {
            *x = x.saturating_add(shift_x);
            *y = y.saturating_add(shift_y);
        }
    }

    FlowchartRenderOutput {
        nodes,
        subgraphs,
        edges,
        width: (i32::from(max_x) - i32::from(min_x)).clamp(0, u16::MAX as i32) as u16,
        height: (i32::from(max_y) - i32::from(min_y)).clamp(0, u16::MAX as i32) as u16,
    }
}

fn include_rect(rect: Rect, min_x: &mut i16, min_y: &mut i16, max_x: &mut i16, max_y: &mut i16) {
    *min_x = (*min_x).min(rect.x);
    *min_y = (*min_y).min(rect.y);
    *max_x = (*max_x).max(rect.x.saturating_add(rect.w as i16));
    *max_y = (*max_y).max(rect.y.saturating_add(rect.h as i16));
}

fn intrinsic_node_size(
    label: &str,
    shape: NodeShape,
    max_width: u16,
) -> (u16, u16, Arc<[Arc<str>]>) {
    let lines = wrap_label(label, max_width.max(1));
    let label_width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0)
        .min(u16::MAX as usize) as u16;
    let label_height = lines.len().max(1).min(u16::MAX as usize) as u16;
    let (extra_w, extra_h, min_w, min_h) = match shape {
        NodeShape::Diamond | NodeShape::Hexagon | NodeShape::DoubleCircle => (4, 2, 5, 3),
        NodeShape::Circle => (4, 2, 5, 3),
        NodeShape::Stadium | NodeShape::Cylinder => (4, 2, 6, 3),
        NodeShape::Subroutine => (4, 2, 6, 3),
        NodeShape::Asymmetric => (3, 2, 5, 3),
        NodeShape::Parallelogram
        | NodeShape::ParallelogramAlt
        | NodeShape::Trapezoid
        | NodeShape::TrapezoidAlt => (4, 2, 5, 3),
        NodeShape::Rect | NodeShape::Round => (2, 2, 3, 3),
    };
    (
        label_width.saturating_add(extra_w).max(min_w),
        label_height.saturating_add(extra_h).max(min_h),
        lines.into(),
    )
}

fn wrap_label(label: &str, max_width: u16) -> Vec<Arc<str>> {
    let max = max_width as usize;
    let mut lines = Vec::new();
    for raw_line in label.lines() {
        let mut current = String::new();
        for word in raw_line.split_whitespace() {
            let needed =
                current.chars().count() + usize::from(!current.is_empty()) + word.chars().count();
            if needed > max && !current.is_empty() {
                lines.push(Arc::<str>::from(std::mem::take(&mut current)));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if current.is_empty() {
            lines.push(Arc::<str>::from(""));
        } else {
            lines.push(Arc::<str>::from(current));
        }
    }
    if lines.is_empty() {
        lines.push(Arc::<str>::from(""));
    }
    lines
}

fn subgraph_depth(flowchart: &Flowchart, id: &NodeId) -> usize {
    let mut depth = 0usize;
    let mut current = flowchart
        .subgraphs
        .iter()
        .find(|subgraph| &subgraph.id == id)
        .and_then(|subgraph| subgraph.parent.as_ref());
    while let Some(parent) = current {
        depth = depth.saturating_add(1);
        current = flowchart
            .subgraphs
            .iter()
            .find(|subgraph| &subgraph.id == parent)
            .and_then(|subgraph| subgraph.parent.as_ref());
    }
    depth
}

fn clamp_i32_to_i16(value: i32) -> i16 {
    value.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

#[cfg(test)]
mod tests {
    use crate::core::node::WidgetNode;
    use crate::style::{Color, Style};

    use super::*;
    use crate::widgets::{Edge, FlowchartItemPath, NodeShape};

    #[test]
    fn empty_flowchart_measures_to_chrome_only() {
        assert_eq!(
            measure_flowchart(
                &Flowchart::new(FlowDirection::TopDown)
                    .padding(1)
                    .border(true)
            ),
            (4, 4)
        );
    }

    #[test]
    fn single_edge_two_nodes_topdown() {
        let output = build_flowchart_output(
            &Flowchart::new(FlowDirection::TopDown)
                .node("A", "Start", NodeShape::Rect)
                .node("B", "End", NodeShape::Rect)
                .edge(Edge::solid("A", "B")),
        );
        let a = output
            .nodes
            .iter()
            .find(|node| node.id.as_str() == "A")
            .unwrap();
        let b = output
            .nodes
            .iter()
            .find(|node| node.id.as_str() == "B")
            .unwrap();
        assert!(a.rect.y < b.rect.y);
    }

    #[test]
    fn per_node_hover_style_is_preserved() {
        let output = build_flowchart_output(
            &Flowchart::new(FlowDirection::TopDown)
                .node("A", "Start", NodeShape::Rect)
                .node_hover_style("A", Style::new().fg(Color::Red)),
        );
        let node = output
            .nodes
            .iter()
            .find(|node| node.id.as_str() == "A")
            .unwrap();

        assert_eq!(node.hover_style.fg, Some(Color::Red.into()));
    }

    #[test]
    fn per_node_hover_style_makes_flowchart_hoverable() {
        let flowchart = Flowchart::new(FlowDirection::TopDown)
            .node("A", "Start", NodeShape::Rect)
            .node_hover_style("A", Style::new().fg(Color::Red));
        let node = crate::widgets::internal::FlowchartNode::from(flowchart);

        assert!(node.is_hoverable());
    }

    #[test]
    fn cycle_breaks_back_edge() {
        let output = build_flowchart_output(
            &Flowchart::new(FlowDirection::TopDown)
                .node("A", "A", NodeShape::Rect)
                .node("B", "B", NodeShape::Rect)
                .node("C", "C", NodeShape::Rect)
                .edge(Edge::solid("A", "B"))
                .edge(Edge::solid("B", "C"))
                .edge(Edge::solid("C", "A")),
        );
        assert!(
            output
                .edges
                .iter()
                .any(|edge| { edge.from.as_str() == "C" && edge.to.as_str() == "A" })
        );
        let distinct_y: HashSet<_> = output.nodes.iter().map(|node| node.rect.y).collect();
        assert_eq!(distinct_y.len(), 3);
    }

    #[test]
    fn barycenter_reduces_crossings() {
        let output = build_flowchart_output(
            &Flowchart::new(FlowDirection::TopDown)
                .node("A", "A", NodeShape::Rect)
                .node("B", "B", NodeShape::Rect)
                .node("C", "C", NodeShape::Rect)
                .node("D", "D", NodeShape::Rect)
                .edge(Edge::solid("A", "D"))
                .edge(Edge::solid("B", "C")),
        );
        let c = output
            .nodes
            .iter()
            .find(|node| node.id.as_str() == "C")
            .unwrap();
        let d = output
            .nodes
            .iter()
            .find(|node| node.id.as_str() == "D")
            .unwrap();
        assert!(d.rect.x <= c.rect.x);
    }

    #[test]
    fn dummy_nodes_inserted_for_long_edges() {
        let flowchart = Flowchart::new(FlowDirection::TopDown)
            .node("A", "A", NodeShape::Rect)
            .node("B", "B", NodeShape::Rect)
            .node("C", "C", NodeShape::Rect)
            .edge(Edge::solid("A", "C"))
            .edge(Edge::solid("A", "B"))
            .edge(Edge::solid("B", "C"));
        let id_to_index: HashMap<NodeId, usize> = flowchart
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id.clone(), index))
            .collect();
        let (edges, _) = classify_edges(&flowchart, &id_to_index);
        let mut nodes: Vec<_> = flowchart
            .nodes
            .iter()
            .enumerate()
            .map(|(index, _node)| LayoutNode {
                spec_index: index,
                layer: 0,
                order: index,
                width: 1,
                height: 1,
                label_lines: Arc::new([]),
            })
            .collect();
        assign_layers(&mut nodes, &edges);
        let long = edges
            .iter()
            .find(|edge| {
                flowchart.edges[edge.index].from.as_str() == "A"
                    && flowchart.edges[edge.index].to.as_str() == "C"
            })
            .unwrap();
        assert_eq!(
            nodes[long.to]
                .layer
                .saturating_sub(nodes[long.from].layer)
                .saturating_sub(1),
            1
        );
    }

    #[test]
    fn subgraph_layout_recursive() {
        let output = build_flowchart_output(&Flowchart::new(FlowDirection::TopDown).subgraph(
            "outer",
            "Outer",
            |b| {
                b.node("A", "A", NodeShape::Rect)
                    .subgraph("inner", "Inner", |b| b.node("B", "B", NodeShape::Rect))
            },
        ));
        assert_eq!(output.subgraphs.len(), 2);
        let outer = output
            .subgraphs
            .iter()
            .find(|sub| sub.id.as_str() == "outer")
            .unwrap();
        let inner = output
            .subgraphs
            .iter()
            .find(|sub| sub.id.as_str() == "inner")
            .unwrap();
        assert!(outer.rect.w >= inner.rect.w);
    }

    #[test]
    fn subgraph_rect_excludes_non_member_branch_sibling() {
        let output = build_flowchart_output(
            &Flowchart::new(FlowDirection::TopDown)
                .subgraph("sg", "Group", |b| {
                    b.node("A", "A", NodeShape::Rect)
                        .node("C", "C", NodeShape::Rect)
                        .edge(Edge::solid("A", "C"))
                })
                .node("B", "B", NodeShape::Rect)
                .edge(Edge::solid("A", "B")),
        );
        let subgraph = output
            .subgraphs
            .iter()
            .find(|subgraph| subgraph.id.as_str() == "sg")
            .unwrap();
        let sibling = output
            .nodes
            .iter()
            .find(|node| node.id.as_str() == "B")
            .unwrap();

        assert!(
            !rect_contains_rect(subgraph.rect, sibling.rect),
            "subgraph {:?} should not contain sibling {:?}",
            subgraph.rect,
            sibling.rect
        );
    }

    #[test]
    fn back_edge_route_avoids_non_endpoint_node_rects() {
        let output = build_flowchart_output(
            &Flowchart::new(FlowDirection::TopDown)
                .node("A", "A", NodeShape::Rect)
                .node("B", "B", NodeShape::Rect)
                .node("C", "C", NodeShape::Rect)
                .edge(Edge::solid("A", "B"))
                .edge(Edge::solid("B", "C"))
                .edge(Edge::solid("C", "A")),
        );
        let middle = output
            .nodes
            .iter()
            .find(|node| node.id.as_str() == "B")
            .unwrap();
        let back_edge = output
            .edges
            .iter()
            .find(|edge| edge.from.as_str() == "C" && edge.to.as_str() == "A")
            .unwrap();

        assert!(
            back_edge
                .cells
                .iter()
                .all(|cell| !rect_contains_point(middle.rect, cell.x, cell.y)),
            "back-edge cells should avoid non-endpoint node {:?}: {:?}",
            middle.rect,
            back_edge.cells
        );
    }

    #[test]
    fn merged_source_port_keeps_attachment_bit() {
        let output = build_flowchart_output(
            &Flowchart::new(FlowDirection::TopDown)
                .node("A", "Start", NodeShape::Stadium)
                .node("B", "Decide?", NodeShape::Diamond)
                .node("C", "Do work", NodeShape::Rect)
                .node("D", "End", NodeShape::Stadium)
                .edge(Edge::solid("A", "B"))
                .edge(Edge::solid("B", "C").label("yes"))
                .edge(Edge::thick("C", "D"))
                .edge(Edge::solid("C", "A").label("loop")),
        );
        let c = output
            .nodes
            .iter()
            .find(|node| node.id.as_str() == "C")
            .unwrap();
        let source_port = (
            c.rect.x.saturating_add(c.rect.w as i16 / 2),
            c.rect.y.saturating_add(c.rect.h as i16),
        );
        let merged_bits = output
            .edges
            .iter()
            .flat_map(|edge| &edge.cells)
            .filter(|cell| (cell.x, cell.y) == source_port)
            .fold(0, |bits, cell| bits | cell.bits);

        assert_ne!(
            merged_bits & NORTH,
            0,
            "source port must connect back to source node"
        );
        assert_ne!(
            merged_bits & SOUTH,
            0,
            "C -> D must continue downward from source port"
        );
        assert_ne!(
            merged_bits & (EAST | WEST),
            0,
            "C -> A back-edge should merge horizontally at the same source port"
        );
        assert_ne!(
            crate::widgets::common::box_glyphs::glyph_for_bits(
                merged_bits,
                crate::style::BorderStyle::Plain,
            ),
            '┌',
            "merged source junction must not lose its north attachment and render as a plain elbow"
        );
    }

    #[test]
    fn predecessorless_source_pulls_forward_toward_later_successor() {
        let flowchart = Flowchart::new(FlowDirection::TopDown)
            .node("A", "A", NodeShape::Rect)
            .node("X", "X", NodeShape::Rect)
            .node("B", "B", NodeShape::Rect)
            .node("T", "T", NodeShape::Rect)
            .edge(Edge::solid("A", "X"))
            .edge(Edge::solid("X", "T"))
            .edge(Edge::solid("B", "T"));
        let id_to_index: HashMap<NodeId, usize> = flowchart
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id.clone(), index))
            .collect();
        let (edges, _) = classify_edges(&flowchart, &id_to_index);
        let mut nodes: Vec<_> = flowchart
            .nodes
            .iter()
            .enumerate()
            .map(|(index, _node)| LayoutNode {
                spec_index: index,
                layer: 0,
                order: index,
                width: 1,
                height: 1,
                label_lines: Arc::new([]),
            })
            .collect();

        assign_layers(&mut nodes, &edges);

        let b = id_to_index[&NodeId::from("B")];
        let t = id_to_index[&NodeId::from("T")];
        assert_eq!(nodes[b].layer, nodes[t].layer.saturating_sub(1));
        assert!(nodes[b].layer > 0);
    }

    #[test]
    fn direction_lr_swaps_axes() {
        let td = build_flowchart_output(
            &Flowchart::new(FlowDirection::TopDown)
                .node("A", "A", NodeShape::Rect)
                .node("B", "B", NodeShape::Rect)
                .edge(Edge::solid("A", "B")),
        );
        let lr = build_flowchart_output(
            &Flowchart::new(FlowDirection::LeftRight)
                .node("A", "A", NodeShape::Rect)
                .node("B", "B", NodeShape::Rect)
                .edge(Edge::solid("A", "B")),
        );
        assert!(td.height > td.width / 2);
        assert!(lr.width > lr.height / 2);
    }

    #[test]
    fn hit_test_returns_node_then_edge_then_subgraph() {
        let flowchart = Flowchart::new(FlowDirection::TopDown)
            .subgraph("sg", "Group", |b| b.node("A", "A", NodeShape::Rect))
            .on_node_click(crate::callback::Callback::new(|_| {}));
        let node = crate::widgets::internal::FlowchartNode::from(flowchart);
        let first = node.output.nodes.first().unwrap();
        let hit = node
            .hit_test(first.rect.x as u16, first.rect.y as u16)
            .unwrap()
            .1;
        assert!(matches!(hit, FlowchartItemPath::Node(_)));
        assert!(
            node.hit_test_refinement(
                first.rect.x,
                first.rect.y,
                Rect {
                    x: 0,
                    y: 0,
                    w: 200,
                    h: 200
                }
            )
            .is_some()
        );
    }

    #[test]
    fn node_shape_diamond_intrinsic_size() {
        let (w, h, _) = intrinsic_node_size("x", NodeShape::Diamond, 20);
        assert!(w >= 5);
        assert!(h >= 3);
    }

    #[test]
    fn edge_between_subgraph_siblings_does_not_cross_inner_subgraph_rect() {
        let output = build_flowchart_output(&Flowchart::new(FlowDirection::TopDown).subgraph(
            "processing",
            "Processing",
            |b| {
                b.node("X", "Step 1", NodeShape::Rect)
                    .node("Y", "Step 2", NodeShape::Rect)
                    .edge(Edge::solid("X", "Y"))
                    .subgraph("nested", "Nested", |b| {
                        b.node("Z", "Inner", NodeShape::Rect)
                    })
            },
        ));
        let nested = output
            .subgraphs
            .iter()
            .find(|sub| sub.id.as_str() == "nested")
            .unwrap();
        let edge = output
            .edges
            .iter()
            .find(|edge| edge.from.as_str() == "X" && edge.to.as_str() == "Y")
            .unwrap();
        for cell in &edge.cells {
            assert!(
                !rect_contains_point(nested.rect, cell.x, cell.y),
                "edge cell {:?} should not lie inside Nested subgraph rect {:?}",
                cell,
                nested.rect
            );
        }
    }

    #[test]
    fn every_visible_edge_produces_cells() {
        let output = build_flowchart_output(
            &Flowchart::new(FlowDirection::TopDown)
                .node("A", "A", NodeShape::Rect)
                .node("B", "B", NodeShape::Rect)
                .node("C", "C", NodeShape::Rect)
                .edge(Edge::solid("A", "B"))
                .edge(Edge::dashed("A", "C"))
                .edge(Edge::thick("B", "C")),
        );
        for edge in &output.edges {
            if matches!(edge.style, EdgeStyle::Invisible) {
                continue;
            }
            assert!(
                !edge.cells.is_empty(),
                "edge {} -> {} produced no cells",
                edge.from,
                edge.to
            );
        }
    }

    #[test]
    fn overlapping_edges_share_cell_bits_for_junction_glyph_compose() {
        // Two forward edges B→D and C→D should both end at D's top-center port,
        // contributing NORTH bits to the same cell. The renderer ORs bits across edges,
        // so this regression asserts the per-edge cell bits are populated (so the merge
        // pass has something to OR — without bits the renderer would draw nothing).
        let output = build_flowchart_output(
            &Flowchart::new(FlowDirection::TopDown)
                .node("A", "A", NodeShape::Rect)
                .node("B", "B", NodeShape::Rect)
                .node("C", "C", NodeShape::Rect)
                .node("D", "D", NodeShape::Rect)
                .edge(Edge::solid("A", "B"))
                .edge(Edge::solid("A", "C"))
                .edge(Edge::solid("B", "D"))
                .edge(Edge::solid("C", "D")),
        );
        for edge in &output.edges {
            for cell in &edge.cells {
                assert!(cell.bits != 0, "edge cell {:?} has no direction bits", cell);
            }
        }
    }

    #[test]
    fn back_edge_route_avoids_non_containing_subgraph_rect() {
        // C → A back-edge wraps around. Processing subgraph contains neither endpoint
        // and must not be crossed by the back-edge cells.
        let output = build_flowchart_output(
            &Flowchart::new(FlowDirection::TopDown)
                .node("A", "Start", NodeShape::Rect)
                .node("C", "End", NodeShape::Rect)
                .edge(Edge::solid("A", "C"))
                .edge(Edge::solid("C", "A"))
                .subgraph("processing", "Processing", |b| {
                    b.node("X", "Step 1", NodeShape::Rect)
                        .node("Y", "Step 2", NodeShape::Rect)
                        .edge(Edge::solid("X", "Y"))
                }),
        );
        let processing = output
            .subgraphs
            .iter()
            .find(|sub| sub.id.as_str() == "processing")
            .unwrap();
        let back = output
            .edges
            .iter()
            .find(|edge| edge.from.as_str() == "C" && edge.to.as_str() == "A")
            .unwrap();
        for cell in &back.cells {
            assert!(
                !rect_contains_point(processing.rect, cell.x, cell.y),
                "back-edge cell {:?} crosses Processing rect {:?}",
                cell,
                processing.rect
            );
        }
    }

    #[test]
    fn back_edge_label_does_not_overlap_routing_cells() {
        let output = build_flowchart_output(
            &Flowchart::new(FlowDirection::TopDown)
                .node("A", "A", NodeShape::Rect)
                .node("B", "B", NodeShape::Rect)
                .node("C", "C", NodeShape::Rect)
                .edge(Edge::solid("A", "B"))
                .edge(Edge::solid("B", "C"))
                .edge(Edge::solid("C", "A").label("loop")),
        );
        let back = output
            .edges
            .iter()
            .find(|edge| edge.from.as_str() == "C" && edge.to.as_str() == "A")
            .unwrap();
        let (lx, ly) = back.label_pos.expect("back-edge label_pos populated");
        let label_width = back.label.as_ref().unwrap().chars().count() as i16;
        for cell in &back.cells {
            let on_label_row = cell.y == ly;
            let in_label_span = cell.x >= lx && cell.x < lx.saturating_add(label_width);
            assert!(
                !(on_label_row && in_label_span),
                "label at ({}, {}) +{} overlaps cell {:?}",
                lx,
                ly,
                label_width,
                cell,
            );
        }
    }

    fn rect_contains_rect(outer: Rect, inner: Rect) -> bool {
        let outer_right = outer.x.saturating_add(outer.w as i16);
        let outer_bottom = outer.y.saturating_add(outer.h as i16);
        let inner_right = inner.x.saturating_add(inner.w as i16);
        let inner_bottom = inner.y.saturating_add(inner.h as i16);
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner_right <= outer_right
            && inner_bottom <= outer_bottom
    }

    fn rect_contains_point(rect: Rect, x: i16, y: i16) -> bool {
        x >= rect.x
            && y >= rect.y
            && x < rect.x.saturating_add(rect.w as i16)
            && y < rect.y.saturating_add(rect.h as i16)
    }
}
