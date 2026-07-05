use std::collections::HashMap;
use std::sync::Arc;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::style::Rect;
use crate::widgets::common::box_glyphs::{
    self, ALL_DIRECTIONS, EAST as E, NORTH as N, SOUTH as S, WEST as W,
};
use crate::widgets::{Graph, GraphDirection, GraphNode, GraphNodePath};

use super::node::{GraphEdgeCell, GraphRenderOutput, PositionedGraphNode};

#[derive(Clone, Debug)]
struct RawNode {
    node: GraphNode,
    sibling_pos: i32,
    depth_pos: i32,
    width: u16,
    height: u16,
    label_lines: Arc<[Arc<str>]>,
    children: Vec<RawNode>,
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: i32,
    max: i32,
}

pub fn measure_graph(graph: &Graph) -> (u16, u16) {
    let output = build_graph_output(graph);
    let mut w = output.width.saturating_add(graph.padding.horizontal());
    let mut h = output.height.saturating_add(graph.padding.vertical());

    if graph.border {
        w = w.saturating_add(2);
        h = h.saturating_add(2);
    }

    (w, h)
}

pub(crate) fn build_graph_output(graph: &Graph) -> GraphRenderOutput {
    let Some(root) = graph.root.as_ref() else {
        return GraphRenderOutput::default();
    };

    let metrics = Metrics::from_root(root, graph);
    let raw = layout_subtree(root, graph, &metrics, 0, 0).0;
    let mut edge_bits = HashMap::<(i16, i16), u8>::new();
    let mut positioned = Vec::new();
    flatten_nodes(
        &raw,
        graph,
        &mut positioned,
        &mut edge_bits,
        None,
        Vec::new(),
    );
    let mut edges: Vec<GraphEdgeCell> = edge_bits
        .into_iter()
        .filter_map(|((x, y), bits)| {
            let bits = bits & ALL_DIRECTIONS;
            (bits != 0).then(|| GraphEdgeCell {
                x,
                y,
                glyph: box_glyphs::glyph_for_bits(bits, graph.edge_border_style),
            })
        })
        .collect();

    normalize_output(&mut positioned, &mut edges)
}

#[derive(Clone, Copy, Debug)]
struct Metrics {
    max_width: u16,
    max_height: u16,
}

impl Metrics {
    fn from_root(root: &GraphNode, graph: &Graph) -> Self {
        let mut metrics = Self {
            max_width: 0,
            max_height: 0,
        };
        visit_metrics(root, graph, &mut metrics);
        metrics
    }
}

fn visit_metrics(node: &GraphNode, graph: &Graph, metrics: &mut Metrics) {
    let (w, h, _) = node_size(node, graph);
    metrics.max_width = metrics.max_width.max(w);
    metrics.max_height = metrics.max_height.max(h);
    for child in node.children.iter() {
        visit_metrics(child, graph, metrics);
    }
}

fn layout_subtree(
    node: &GraphNode,
    graph: &Graph,
    metrics: &Metrics,
    depth: u16,
    cursor: i32,
) -> (RawNode, Bounds) {
    let (width, height, label_lines) = node_size(node, graph);
    let (sibling_size, sibling_gap, depth_size, depth_gap) = match graph.direction {
        GraphDirection::TopDown => (width, graph.gap_x, metrics.max_height, graph.gap_y),
        GraphDirection::LeftRight => (height, graph.gap_y, metrics.max_width, graph.gap_x),
    };

    let depth_pos = i32::from(depth) * i32::from(depth_size.saturating_add(depth_gap));
    let mut child_cursor = cursor;
    let mut children = Vec::with_capacity(node.children.len());
    let mut child_min = i32::MAX;
    let mut child_max = i32::MIN;

    // This uses each child subtree's full axis-aligned bounds as its contour. It is safe
    // and deterministic, but intentionally less tight than a full Reingold-Tilford
    // contour-pair offset pass for asymmetric deep trees.
    for child in node.children.iter() {
        let (child_raw, child_bounds) =
            layout_subtree(child, graph, metrics, depth.saturating_add(1), child_cursor);
        child_cursor = child_bounds.max.saturating_add(i32::from(sibling_gap));
        child_min = child_min.min(child_bounds.min);
        child_max = child_max.max(child_bounds.max);
        children.push(child_raw);
    }

    let sibling_pos = if children.is_empty() {
        cursor
    } else {
        let center = child_min.saturating_add(child_max) / 2;
        center.saturating_sub(i32::from(sibling_size) / 2)
    };

    let mut raw = RawNode {
        node: node.clone(),
        sibling_pos,
        depth_pos,
        width,
        height,
        label_lines,
        children,
    };

    let raw_span = i32::from(sibling_size);
    let mut min = if child_min == i32::MAX {
        sibling_pos
    } else {
        child_min
    }
    .min(sibling_pos);
    let mut max = if child_max == i32::MIN {
        sibling_pos + raw_span
    } else {
        child_max
    }
    .max(sibling_pos.saturating_add(raw_span));

    if min < cursor {
        let delta = cursor.saturating_sub(min);
        shift_subtree(&mut raw, delta);
        min = min.saturating_add(delta);
        max = max.saturating_add(delta);
    }

    (raw, Bounds { min, max })
}

fn shift_subtree(raw: &mut RawNode, delta: i32) {
    raw.sibling_pos = raw.sibling_pos.saturating_add(delta);
    for child in &mut raw.children {
        shift_subtree(child, delta);
    }
}

fn flatten_nodes(
    raw: &RawNode,
    graph: &Graph,
    out: &mut Vec<PositionedGraphNode>,
    edge_bits: &mut HashMap<(i16, i16), u8>,
    parent_rect: Option<Rect>,
    path: Vec<usize>,
) {
    let (x, y) = match graph.direction {
        GraphDirection::TopDown => (raw.sibling_pos, raw.depth_pos),
        GraphDirection::LeftRight => (raw.depth_pos, raw.sibling_pos),
    };
    let rect = Rect {
        x: clamp_i32_to_i16(x),
        y: clamp_i32_to_i16(y),
        w: raw.width,
        h: raw.height,
    };
    let positioned = PositionedGraphNode {
        rect,
        path: GraphNodePath::from(path.clone()),
        label: raw.node.label.clone(),
        label_lines: raw.label_lines.clone(),
        style: raw.node.style,
        hover_style: raw.node.hover_style,
        focus_style: raw.node.focus_style,
        border: raw.node.border.unwrap_or(graph.node_border),
    };

    if let Some(parent_rect) = parent_rect {
        route_edge(parent_rect, positioned.rect, graph.direction, edge_bits);
    }

    let current_rect = positioned.rect;
    out.push(positioned);

    for (index, child) in raw.children.iter().enumerate() {
        let mut child_path = path.clone();
        child_path.push(index);
        flatten_nodes(child, graph, out, edge_bits, Some(current_rect), child_path);
    }
}

fn normalize_output(
    nodes: &mut [PositionedGraphNode],
    edges: &mut [GraphEdgeCell],
) -> GraphRenderOutput {
    let mut min_x = i16::MAX;
    let mut min_y = i16::MAX;
    let mut max_x = i16::MIN;
    let mut max_y = i16::MIN;

    for node in nodes.iter() {
        min_x = min_x.min(node.rect.x);
        min_y = min_y.min(node.rect.y);
        max_x = max_x.max(node.rect.x.saturating_add(node.rect.w as i16));
        max_y = max_y.max(node.rect.y.saturating_add(node.rect.h as i16));
    }
    for edge in edges.iter() {
        min_x = min_x.min(edge.x);
        min_y = min_y.min(edge.y);
        max_x = max_x.max(edge.x.saturating_add(1));
        max_y = max_y.max(edge.y.saturating_add(1));
    }

    if min_x == i16::MAX || min_y == i16::MAX {
        return GraphRenderOutput::default();
    }

    let shift_x = 0i16.saturating_sub(min_x);
    let shift_y = 0i16.saturating_sub(min_y);
    for node in nodes.iter_mut() {
        node.rect.x = node.rect.x.saturating_add(shift_x);
        node.rect.y = node.rect.y.saturating_add(shift_y);
    }
    for edge in edges.iter_mut() {
        edge.x = edge.x.saturating_add(shift_x);
        edge.y = edge.y.saturating_add(shift_y);
    }

    GraphRenderOutput {
        nodes: nodes.to_vec(),
        edges: edges.to_vec(),
        width: (i32::from(max_x) - i32::from(min_x)).clamp(0, u16::MAX as i32) as u16,
        height: (i32::from(max_y) - i32::from(min_y)).clamp(0, u16::MAX as i32) as u16,
    }
}

fn node_size(node: &GraphNode, graph: &Graph) -> (u16, u16, Arc<[Arc<str>]>) {
    let border = node.border.unwrap_or(graph.node_border);
    let label_lines = wrap_label(node.label.as_ref(), graph.max_node_width);
    let label_width = widest_label_line(&label_lines);
    let label_height = label_lines.len().max(1).min(u16::MAX as usize) as u16;
    let mut width = label_width
        .saturating_add(graph.node_padding.horizontal())
        .max(1);
    let mut height = label_height.saturating_add(graph.node_padding.vertical());
    if border {
        width = width.saturating_add(2);
        height = height.saturating_add(2);
    }
    (width, height, label_lines)
}

fn wrap_label(text: &str, max_width: u16) -> Arc<[Arc<str>]> {
    let max_width = max_width.max(1) as usize;
    let mut lines = Vec::new();
    for source_line in text.lines() {
        wrap_source_line(source_line, max_width, &mut lines);
    }
    if lines.is_empty() {
        lines.push(Arc::<str>::from(""));
    }
    lines.into()
}

fn wrap_source_line(source: &str, max_width: usize, out: &mut Vec<Arc<str>>) {
    if source.is_empty() {
        out.push(Arc::<str>::from(""));
        return;
    }

    let mut current = String::new();
    let mut current_width = 0usize;
    for word in source.split_whitespace() {
        let word_width = UnicodeWidthStr::width(word);
        if current_width > 0
            && current_width.saturating_add(1).saturating_add(word_width) <= max_width
        {
            current.push(' ');
            current.push_str(word);
            current_width = current_width.saturating_add(1).saturating_add(word_width);
        } else if current_width == 0 && word_width <= max_width {
            current.push_str(word);
            current_width = word_width;
        } else {
            if !current.is_empty() {
                out.push(Arc::<str>::from(std::mem::take(&mut current)));
                current_width = 0;
            }
            push_wrapped_word(word, max_width, out, &mut current, &mut current_width);
        }
    }

    if !current.is_empty() {
        out.push(Arc::<str>::from(current));
    }
}

fn push_wrapped_word(
    word: &str,
    max_width: usize,
    out: &mut Vec<Arc<str>>,
    current: &mut String,
    current_width: &mut usize,
) {
    for ch in word.chars() {
        let width = ch.width().unwrap_or(0);
        if *current_width > 0 && current_width.saturating_add(width) > max_width {
            out.push(Arc::<str>::from(std::mem::take(current)));
            *current_width = 0;
        }
        current.push(ch);
        *current_width = current_width.saturating_add(width);
    }
}

fn widest_label_line(lines: &[Arc<str>]) -> u16 {
    lines
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_ref()).min(u16::MAX as usize) as u16)
        .max()
        .unwrap_or(0)
}

fn route_edge(
    parent: Rect,
    child: Rect,
    direction: GraphDirection,
    edge_bits: &mut HashMap<(i16, i16), u8>,
) {
    match direction {
        GraphDirection::TopDown => {
            let start = (
                parent.x.saturating_add(parent.w as i16 / 2),
                parent.y.saturating_add(parent.h as i16),
            );
            let end = (
                child.x.saturating_add(child.w as i16 / 2),
                child.y.saturating_sub(1),
            );
            if start.1 > end.1 {
                return;
            }
            *edge_bits.entry(start).or_insert(0) |= N;
            *edge_bits.entry(end).or_insert(0) |= S;
            if start == end {
                return;
            }
            let mid_y = start.1.saturating_add((end.1.saturating_sub(start.1)) / 2);
            connect(start, (start.0, mid_y), edge_bits);
            connect((start.0, mid_y), (end.0, mid_y), edge_bits);
            connect((end.0, mid_y), end, edge_bits);
        }
        GraphDirection::LeftRight => {
            let start = (
                parent.x.saturating_add(parent.w as i16),
                parent.y.saturating_add(parent.h as i16 / 2),
            );
            let end = (
                child.x.saturating_sub(1),
                child.y.saturating_add(child.h as i16 / 2),
            );
            if start.0 > end.0 {
                return;
            }
            *edge_bits.entry(start).or_insert(0) |= W;
            *edge_bits.entry(end).or_insert(0) |= E;
            if start == end {
                return;
            }
            let mid_x = start.0.saturating_add((end.0.saturating_sub(start.0)) / 2);
            connect(start, (mid_x, start.1), edge_bits);
            connect((mid_x, start.1), (mid_x, end.1), edge_bits);
            connect((mid_x, end.1), end, edge_bits);
        }
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
                bits |= N;
            }
            if y < end {
                bits |= S;
            }
            *edge_bits.entry((a.0, y)).or_insert(0) |= bits;
        }
    } else if a.1 == b.1 {
        let (start, end) = if a.0 <= b.0 { (a.0, b.0) } else { (b.0, a.0) };
        for x in start..=end {
            let mut bits = 0;
            if x > start {
                bits |= W;
            }
            if x < end {
                bits |= E;
            }
            *edge_bits.entry((x, a.1)).or_insert(0) |= bits;
        }
    }
}

fn clamp_i32_to_i16(value: i32) -> i16 {
    value.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

#[cfg(test)]
mod tests {
    use crate::core::node::WidgetNode;
    use crate::style::{BorderStyle, Color, Style};

    use super::*;

    #[test]
    fn empty_graph_measures_to_chrome_only() {
        assert_eq!(measure_graph(&Graph::new().padding(1).border(true)), (4, 4));
    }

    #[test]
    fn single_node_has_no_edges() {
        let output = build_graph_output(&Graph::new().root(GraphNode::new("A")));
        assert_eq!(output.nodes.len(), 1);
        assert!(output.edges.is_empty());
    }

    #[test]
    fn center_offset_for_centers_requested_node() {
        let graph = Graph::new()
            .root(GraphNode::new("A").child(GraphNode::new("B")))
            .direction(GraphDirection::LeftRight);
        let output = build_graph_output(&graph);

        for node in &output.nodes {
            let offset = graph
                .center_offset_for(&node.path, 40, 20)
                .expect("known node centers");
            let center_x = i32::from(node.rect.x) + i32::from(node.rect.w) / 2;
            let center_y = i32::from(node.rect.y) + i32::from(node.rect.h) / 2;
            assert_eq!(offset, (center_x - 20, center_y - 10));
        }

        assert!(
            graph
                .center_offset_for(&GraphNodePath::from_segments([9]), 40, 20)
                .is_none()
        );
    }

    #[test]
    fn parent_child_emits_edge_cells() {
        let output =
            build_graph_output(&Graph::new().root(GraphNode::new("A").child(GraphNode::new("B"))));
        assert_eq!(output.nodes.len(), 2);
        assert!(!output.edges.is_empty());
    }

    #[test]
    fn node_labels_wrap_to_multiple_lines() {
        let output = build_graph_output(
            &Graph::new()
                .root(GraphNode::new("Alpha Beta Gamma"))
                .max_node_width(5),
        );
        let node = &output.nodes[0];

        assert_eq!(node.rect.w, 9);
        assert_eq!(node.rect.h, 5);
        assert_eq!(node.label_lines.len(), 3);
        assert_eq!(node.label_lines[0].as_ref(), "Alpha");
        assert_eq!(node.label_lines[1].as_ref(), "Beta");
        assert_eq!(node.label_lines[2].as_ref(), "Gamma");
    }

    #[test]
    fn node_labels_honor_explicit_newlines() {
        let output = build_graph_output(&Graph::new().root(GraphNode::new("A\nB")));
        let node = &output.nodes[0];

        assert_eq!(node.rect.h, 4);
        assert_eq!(node.label_lines.len(), 2);
        assert_eq!(node.label_lines[0].as_ref(), "A");
        assert_eq!(node.label_lines[1].as_ref(), "B");
    }

    #[test]
    fn per_node_hover_style_is_preserved() {
        let output = build_graph_output(
            &Graph::new().root(GraphNode::new("root").hover_style(Style::new().fg(Color::Red))),
        );
        let node = &output.nodes[0];

        assert_eq!(node.hover_style.fg, Some(Color::Red.into()));
    }

    #[test]
    fn per_node_focus_style_is_preserved() {
        let output = build_graph_output(
            &Graph::new().root(GraphNode::new("root").focus_style(Style::new().fg(Color::Red))),
        );
        let node = &output.nodes[0];

        assert_eq!(
            node.focus_style.explicit_style().and_then(|style| style.fg),
            Some(Color::Red.into()),
        );
    }

    #[test]
    fn per_node_hover_style_makes_graph_hoverable() {
        let graph =
            Graph::new().root(GraphNode::new("root").hover_style(Style::new().fg(Color::Red)));
        let node = crate::widgets::internal::GraphRenderNode::from(graph);
        let rect = Rect {
            x: 0,
            y: 0,
            w: node.output.width,
            h: node.output.height,
        };

        assert!(node.is_hoverable());
        assert_eq!(node.hover_test_refinement(0, 0, rect), Some(true));
    }

    #[test]
    fn rounded_edge_style_uses_rounded_elbows() {
        let output = build_graph_output(
            &Graph::new()
                .root(
                    GraphNode::new("A")
                        .child(GraphNode::new("B"))
                        .child(GraphNode::new("C")),
                )
                .edge_border_style(BorderStyle::Rounded),
        );

        assert!(
            output
                .edges
                .iter()
                .any(|edge| matches!(edge.glyph, '╭' | '╮' | '╰' | '╯'))
        );
    }

    #[test]
    fn hit_test_returns_correct_path() {
        let graph = Graph::new().root(
            GraphNode::new("root")
                .child(GraphNode::new("left"))
                .child(GraphNode::new("right")),
        );
        let node = crate::widgets::internal::GraphRenderNode::from(graph);
        let right = node
            .output
            .nodes
            .iter()
            .find(|node| node.label.as_ref() == "right")
            .expect("right node should be positioned");

        let hit = node
            .hit_test(right.rect.x as u16, right.rect.y as u16)
            .expect("right node should be hit")
            .1;

        assert_eq!(hit.path.as_ref(), &[1]);
    }

    #[test]
    fn hit_test_misses_outside_nodes() {
        let graph = Graph::new().root(GraphNode::new("root"));
        let node = crate::widgets::internal::GraphRenderNode::from(graph);

        assert!(node.hit_test(u16::MAX, u16::MAX).is_none());
    }

    #[test]
    fn hover_only_graph_does_not_refine_click_hit_testing() {
        let graph = Graph::new()
            .root(GraphNode::new("root"))
            .node_hover_style(Style::new().bg(Color::Blue));
        let node = crate::widgets::internal::GraphRenderNode::from(graph);
        let rect = Rect {
            x: 0,
            y: 0,
            w: node.output.width,
            h: node.output.height,
        };

        assert_eq!(node.hit_test_refinement(0, 0, rect), None);
        assert_eq!(node.hover_test_refinement(0, 0, rect), Some(true));
        assert_eq!(node.hover_test_refinement(10, 10, rect), Some(false));
    }
}
