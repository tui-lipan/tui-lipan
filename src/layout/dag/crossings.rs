use super::{WorkingEdge, WorkingNode};

pub(super) fn reduce_crossings(nodes: &mut [WorkingNode], edges: &[WorkingEdge]) {
    normalize_orders(nodes);
    let max_layer = nodes.iter().map(|node| node.layer).max().unwrap_or(0);

    for sweep in 0..8 {
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
            layer_nodes.sort_by(|&a, &b| {
                barycenter(a, nodes, edges, sweep % 2 == 0)
                    .total_cmp(&barycenter(b, nodes, edges, sweep % 2 == 0))
                    .then_with(|| nodes[a].order.cmp(&nodes[b].order))
                    .then_with(|| nodes[a].spec_index.cmp(&nodes[b].spec_index))
            });
            for (order, index) in layer_nodes.into_iter().enumerate() {
                nodes[index].order = order;
            }
        }
    }
    normalize_orders(nodes);
}

fn normalize_orders(nodes: &mut [WorkingNode]) {
    let max_layer = nodes.iter().map(|node| node.layer).max().unwrap_or(0);
    for layer in 0..=max_layer {
        let mut indices: Vec<_> = nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| (node.layer == layer).then_some(index))
            .collect();
        indices.sort_by_key(|&index| (nodes[index].order, nodes[index].spec_index));
        for (order, index) in indices.into_iter().enumerate() {
            nodes[index].order = order;
        }
    }
}

fn barycenter(index: usize, nodes: &[WorkingNode], edges: &[WorkingEdge], incoming: bool) -> f32 {
    let mut total = 0f32;
    let mut count = 0f32;
    for edge in edges.iter().filter(|edge| !edge.reversed) {
        let neighbor = if incoming && edge.to == index {
            Some(edge.from)
        } else if !incoming && edge.from == index {
            Some(edge.to)
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
