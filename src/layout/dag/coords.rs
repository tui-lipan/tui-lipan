use crate::style::Rect;

use super::{DagInput, PositionedNode, RoutedEdge, WorkingNode};

pub(super) fn assign_coordinates(input: &DagInput, nodes: &[WorkingNode]) -> Vec<PositionedNode> {
    let max_layer = nodes.iter().map(|node| node.layer).max().unwrap_or(0);
    let mut layer_heights = vec![0u16; max_layer.saturating_add(1)];
    let mut layer_widths = vec![0u16; max_layer.saturating_add(1)];

    for node in nodes {
        layer_heights[node.layer] = layer_heights[node.layer].max(node.height);
        if layer_widths[node.layer] > 0 {
            layer_widths[node.layer] =
                layer_widths[node.layer].saturating_add(input.options.node_gap);
        }
        layer_widths[node.layer] = layer_widths[node.layer].saturating_add(node.width);
    }

    let widest_layer = layer_widths.iter().copied().max().unwrap_or(0);
    let mut layer_y = vec![input.options.margin_y as i16; max_layer.saturating_add(1)];
    for layer in 1..=max_layer {
        layer_y[layer] = layer_y[layer - 1]
            .saturating_add(layer_heights[layer - 1] as i16)
            .saturating_add(input.options.layer_gap as i16);
    }

    let mut positioned = Vec::with_capacity(nodes.len());
    for layer in 0..=max_layer {
        let mut layer_nodes: Vec<_> = nodes.iter().filter(|node| node.layer == layer).collect();
        layer_nodes.sort_by_key(|node| (node.order, node.spec_index));
        let mut x = input.options.margin_x as i16
            + ((widest_layer.saturating_sub(layer_widths[layer])) / 2) as i16;
        for node in layer_nodes {
            let spec = &input.nodes[node.spec_index];
            let y =
                layer_y[layer] + ((layer_heights[layer].saturating_sub(node.height)) / 2) as i16;
            positioned.push(PositionedNode {
                id: spec.id.clone(),
                rect: Rect {
                    x,
                    y,
                    w: node.width,
                    h: node.height,
                },
                layer: node.layer,
                order: node.order,
                group: spec.group.clone(),
            });
            x = x
                .saturating_add(node.width as i16)
                .saturating_add(input.options.node_gap as i16);
        }
    }
    positioned.sort_by_key(|node| (node.layer, node.order, node.id.clone()));
    positioned
}

pub(super) fn bounds(nodes: &[PositionedNode], edges: &[RoutedEdge]) -> Rect {
    let mut min_x = 0i16;
    let mut min_y = 0i16;
    let mut max_x = 0i16;
    let mut max_y = 0i16;

    for node in nodes {
        min_x = min_x.min(node.rect.x);
        min_y = min_y.min(node.rect.y);
        max_x = max_x.max(node.rect.x.saturating_add(node.rect.w as i16));
        max_y = max_y.max(node.rect.y.saturating_add(node.rect.h as i16));
    }
    for edge in edges {
        for point in &edge.points {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x.saturating_add(1));
            max_y = max_y.max(point.y.saturating_add(1));
        }
    }

    Rect {
        x: min_x,
        y: min_y,
        w: (max_x - min_x).max(0) as u16,
        h: (max_y - min_y).max(0) as u16,
    }
}
