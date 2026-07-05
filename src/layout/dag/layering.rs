use std::collections::{HashMap, HashSet};

use super::{DagInput, DagNode, DagNodeId, WorkingEdge, WorkingNode};

pub(super) fn classify_edges(input: &DagInput, node_count: usize) -> Vec<WorkingEdge> {
    let id_to_index: HashMap<&DagNodeId, usize> = input
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (&node.id, index))
        .collect();
    let mut adjacency = vec![Vec::<(usize, usize)>::new(); node_count];
    let mut raw_edges = Vec::new();

    for (edge_index, edge) in input.edges.iter().enumerate() {
        let (Some(&from), Some(&to)) = (id_to_index.get(&edge.from), id_to_index.get(&edge.to))
        else {
            continue;
        };
        raw_edges.push((edge_index, from, to));
        adjacency[from].push((to, edge_index));
    }

    let back_edges = detect_back_edges(&adjacency);
    raw_edges
        .into_iter()
        .map(|(spec_index, from, to)| WorkingEdge {
            spec_index,
            from,
            to,
            reversed: back_edges.contains(&spec_index),
        })
        .collect()
}

fn detect_back_edges(adjacency: &[Vec<(usize, usize)>]) -> HashSet<usize> {
    fn visit(
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
                visit(next, adjacency, visiting, visited, back_edges);
            }
        }
        visiting[node] = false;
        visited[node] = true;
    }

    let mut visiting = vec![false; adjacency.len()];
    let mut visited = vec![false; adjacency.len()];
    let mut back_edges = HashSet::new();
    for node in 0..adjacency.len() {
        visit(
            node,
            adjacency,
            &mut visiting,
            &mut visited,
            &mut back_edges,
        );
    }
    back_edges
}

pub(super) fn assign_layers(
    nodes: &mut [WorkingNode],
    edges: &mut [WorkingEdge],
    specs: &[DagNode],
) {
    let mut pass = 0usize;
    loop {
        reset_layers(nodes, specs);
        propagate_layers(nodes, edges, specs);
        pull_predecessorless_sources_forward(nodes, edges);
        if !promote_stretched_feedback_edges(nodes, edges, specs) {
            break;
        }
        pass = pass.saturating_add(1);
        if pass >= edges.len().max(1) {
            break;
        }
    }
}

fn reset_layers(nodes: &mut [WorkingNode], specs: &[DagNode]) {
    for (node, spec) in nodes.iter_mut().zip(specs) {
        node.layer = spec.layer_hint.unwrap_or(0);
    }
}

fn propagate_layers(nodes: &mut [WorkingNode], edges: &[WorkingEdge], specs: &[DagNode]) {
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

        for (index, spec) in specs.iter().enumerate() {
            if let Some(hint) = spec.layer_hint
                && nodes[index].layer < hint
            {
                nodes[index].layer = hint;
                changed = true;
            }
        }
    }
}

fn promote_stretched_feedback_edges(
    nodes: &[WorkingNode],
    edges: &mut [WorkingEdge],
    specs: &[DagNode],
) -> bool {
    let mut outgoing_counts = vec![0usize; nodes.len()];
    let mut incoming = vec![Vec::<usize>::new(); nodes.len()];
    for (edge_index, edge) in edges.iter().enumerate().filter(|(_, edge)| !edge.reversed) {
        outgoing_counts[edge.from] = outgoing_counts[edge.from].saturating_add(1);
        incoming[edge.to].push(edge_index);
    }

    let mut to_reverse = Vec::new();
    for target in 0..nodes.len() {
        if outgoing_counts[target] == 0 || incoming[target].len() < 2 {
            continue;
        }

        for &edge_index in &incoming[target] {
            let edge = &edges[edge_index];
            let source = edge.from;
            if nodes[target].spec_index >= nodes[source].spec_index {
                continue;
            }
            if outgoing_counts[source] > 1 {
                continue;
            }
            if nodes[source].layer.saturating_add(1) != nodes[target].layer {
                continue;
            }

            let Some(other_min_pred_layer) = incoming[target]
                .iter()
                .copied()
                .filter(|other_index| *other_index != edge_index)
                .map(|other_index| nodes[edges[other_index].from].layer)
                .min()
            else {
                continue;
            };

            let compact_layer = specs[target]
                .layer_hint
                .unwrap_or(0)
                .max(other_min_pred_layer.saturating_add(1));
            if nodes[target].layer >= compact_layer.saturating_add(2)
                && nodes[source].layer >= compact_layer
            {
                to_reverse.push(edge_index);
            }
        }
    }

    to_reverse.sort_unstable();
    to_reverse.dedup();
    for edge_index in &to_reverse {
        edges[*edge_index].reversed = true;
    }
    !to_reverse.is_empty()
}

fn pull_predecessorless_sources_forward(nodes: &mut [WorkingNode], edges: &[WorkingEdge]) {
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
